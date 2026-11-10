//! Join provider transitions to the configuration receipt without blocking requests.
use super::*;
use std::collections::HashSet;

#[derive(Serialize, Deserialize)]
struct LoanJournal {
    version: u32,
    config: PathBuf,
    loans: Vec<ThreadLoan>,
}

#[derive(Serialize, Deserialize)]
struct ThreadLoan {
    provider: String,
    recovery_id: String,
    ids: HashSet<String>,
}

fn valid_recovery_id(id: &str) -> bool {
    id.len() == 32 && id.bytes().all(|byte| byte.is_ascii_hexdigit())
}

fn pending(result: Result<threads::ThreadTransitionReport, String>) -> bool {
    result.map_or(true, |report| report.incomplete)
}

impl Manager {
    pub(crate) fn thread_sync_pending(&self) -> bool {
        self.load()
            .ok()
            .flatten()
            .is_some_and(|r| r.thread_sync_pending)
            || self
                .load_thread_loans()
                .map_or(true, |journal| !journal.loans.is_empty())
    }

    pub(crate) fn sync_threads(&self) -> Result<(), String> {
        // A loan outlives Disconnect's configuration receipt. Return only its
        // recorded chats when Codex subsequently selects OpenAI again.
        let config: toml::Value = read(&self.config)?
            .unwrap_or_default()
            .parse()
            .map_err(|_| FAILED)?;
        let loan_pending = self.recover_thread_loans(&effective_provider(&config))?;
        let Some(mut receipt) = self.load()? else {
            return self.lend_connected_provider_threads(&config);
        };
        if receipt.thread_restore_pending {
            return self.restore();
        }
        if read(&self.config)?
            .as_deref()
            .is_some_and(|current| projection_owned(current, &receipt.installed))
        {
            self.reconcile_thread_receipt(&mut receipt, false)?;
            if loan_pending && !receipt.thread_sync_pending {
                receipt.thread_sync_pending = true;
                self.save_thread_receipt(&receipt)?;
            }
        }
        Ok(())
    }

    pub(super) fn reconcile_thread_receipt(
        &self,
        receipt: &mut Receipt,
        restoring: bool,
    ) -> Result<(), String> {
        let original: toml::Value = receipt
            .original
            .as_deref()
            .unwrap_or("")
            .parse()
            .map_err(|_| FAILED)?;
        let installed: toml::Value = receipt.installed.parse().map_err(|_| FAILED)?;
        let source = source_provider(&original);
        let target = effective_provider(&installed);
        let historical = historical_provider_ids(&original, &source);
        // A selected or named profile retains its own provider bucket. Existing
        // journals still recover transitions this receipt actually performed.
        if receipt.thread_recovery_id.is_none()
            && (selected_profile_provider(&installed).is_some()
                || source != "openai" && profile_names_provider(&original, &source))
        {
            return Ok(());
        }
        if receipt.thread_recovery_id.is_none()
            && (restoring && target != PROVIDER
                || source == "openai" && target == "openai" && historical.is_empty()
                || target != PROVIDER && target != "openai" && target != source)
        {
            return Ok(());
        }
        let recovery_id = receipt
            .thread_recovery_id
            .get_or_insert_with(|| uuid::Uuid::new_v4().simple().to_string())
            .clone();
        if !valid_recovery_id(&recovery_id) {
            return Err(CONFLICT.into());
        }
        let home = self.config.parent().ok_or(FAILED)?;
        let recovery = self.thread_recovery_path(&recovery_id)?;
        receipt.thread_restore_pending = restoring;
        receipt.thread_sync_pending = true;
        self.save_thread_receipt(receipt)?;
        let mut incomplete = false;
        if source != "openai" {
            // Recover the outer loan record after an interruption between its
            // row journal and private ledger writes, including during Disconnect.
            for lender in [PROVIDER, source.as_str()] {
                incomplete |= self
                    .record_thread_loans(
                        &recovery.join("lent-routing"),
                        &source,
                        lender,
                        &recovery_id,
                    )
                    .is_err();
            }
        }
        let mut reversal_pending = false;
        // Several retained tables can display the same VaryAI name while naming
        // different provider IDs. Reverse their owned moves before a broad
        // current-provider transition can erase each chat's original bucket.
        for historical_source in &historical {
            for previous_target in [PROVIDER, "openai", source.as_str()]
                .into_iter()
                .collect::<HashSet<_>>()
            {
                if previous_target == historical_source.as_str()
                    || (!restoring && previous_target == target)
                {
                    continue;
                }
                reversal_pending |= pending(threads::restore(
                    home,
                    &recovery
                        .join("historical-routing")
                        .join(format!("provider-{historical_source}"))
                        .join(format!("target-{previous_target}")),
                    historical_source,
                    previous_target,
                ));
            }
        }
        // Keep the historical journal name for interrupted older builds.
        if (restoring || target != PROVIDER) && source != PROVIDER {
            reversal_pending |= pending(threads::restore(
                home,
                &recovery.join("into-routing"),
                &source,
                PROVIDER,
            ));
        }
        if (restoring || target != "openai") && source != "openai" {
            reversal_pending |= pending(threads::restore(
                home,
                &recovery.join("beside-routing"),
                &source,
                "openai",
            ));
        }
        // Finish an owned reversal before another provider pair can claim its
        // partially restored rows. Busy chats are retried by the normal watcher.
        incomplete |= reversal_pending;
        if !reversal_pending {
            if restoring {
                if source != PROVIDER {
                    incomplete |= pending(threads::transition(
                        home,
                        &recovery.join("out-of-routing"),
                        PROVIDER,
                        &source,
                    ));
                }
            } else if target == PROVIDER {
                if source != PROVIDER {
                    incomplete |= pending(threads::transition(
                        home,
                        &recovery.join("into-routing"),
                        &source,
                        PROVIDER,
                    ));
                }
            } else {
                if target != PROVIDER {
                    incomplete |= pending(threads::transition(
                        home,
                        &recovery.join("out-of-routing"),
                        PROVIDER,
                        &target,
                    ));
                }
                if source != target && target == "openai" {
                    incomplete |= pending(threads::transition(
                        home,
                        &recovery.join("beside-routing"),
                        &source,
                        "openai",
                    ));
                }
            }
        }
        if !restoring
            && !reversal_pending
            && source != "openai"
            && target != "openai"
            && !profile_names_provider(&original, "openai")
        {
            // Match Magpie's bringOver/lendOwn: an OpenAI chat must also resume
            // when a user provider remains selected, even with identical labels.
            let journal = recovery.join("lent-routing");
            incomplete |= pending(threads::transition(home, &journal, "openai", &target));
            incomplete |= self
                .record_thread_loans(&journal, &source, &target, &recovery_id)
                .is_err();
        }
        if !restoring && !reversal_pending {
            let current = read(&self.config)?.and_then(|text| text.parse::<toml::Value>().ok());
            for historical_source in &historical {
                if historical_source == &target
                    || current.as_ref().is_none_or(|current| {
                        profile_names_provider(current, historical_source)
                            || current
                                .get("model_providers")
                                .and_then(|tables| tables.get(historical_source))
                                != original
                                    .get("model_providers")
                                    .and_then(|tables| tables.get(historical_source))
                    })
                {
                    continue;
                }
                incomplete |= pending(threads::transition(
                    home,
                    &recovery
                        .join("historical-routing")
                        .join(format!("provider-{historical_source}"))
                        .join(format!("target-{target}")),
                    historical_source,
                    &target,
                ));
            }
        }
        let active = if restoring { &source } else { &target };
        incomplete |= self.recover_thread_loans(active).unwrap_or(true);
        receipt.thread_sync_pending = incomplete;
        self.save_thread_receipt(receipt)
    }

    fn lend_connected_provider_threads(&self, config: &toml::Value) -> Result<(), String> {
        let provider = effective_provider(config);
        if matches!(provider.as_str(), "openai" | PROVIDER)
            || selected_profile_provider(config).is_some()
            || profile_names_provider(config, "openai")
        {
            return Ok(());
        }
        // A displayed label or an arbitrary local URL cannot authorize a
        // receiptless history move. The current runtime's private menu token
        // must own this selected Responses route; Disconnect clears that menu.
        let Some(menu) = self.runtime.menu_snapshot()? else {
            return Ok(());
        };
        let Some(table) = config
            .get("model_providers")
            .and_then(|tables| tables.get(&provider))
        else {
            return Ok(());
        };
        if table
            .get("experimental_bearer_token")
            .and_then(toml::Value::as_str)
            != Some(menu.token.as_str())
            || table.get("wire_api").and_then(toml::Value::as_str) != Some("responses")
            || !table
                .get("base_url")
                .and_then(toml::Value::as_str)
                .and_then(|raw| url::Url::parse(raw).ok())
                .is_some_and(|url| {
                    url.scheme() == "http"
                        && matches!(url.host_str(), Some("127.0.0.1" | "localhost" | "[::1]"))
                        && url.port().is_some_and(|port| port != 0)
                        && matches!(url.path(), "/intent/v1" | "/v1")
                        && url.username().is_empty()
                        && url.password().is_none()
                        && url.query().is_none()
                        && url.fragment().is_none()
                })
        {
            return Ok(());
        }
        let mut journal = self.load_thread_loans()?;
        let recovery_id =
            if let Some(loan) = journal.loans.iter().find(|loan| loan.provider == provider) {
                loan.recovery_id.clone()
            } else {
                let id = uuid::Uuid::new_v4().simple().to_string();
                // Anchor the private row journal before its first write, so a
                // failed subsequent ledger update can be retried without a receipt.
                journal.loans.push(ThreadLoan {
                    provider: provider.clone(),
                    recovery_id: id.clone(),
                    ids: HashSet::new(),
                });
                self.save_thread_loans(&journal)?;
                id
            };
        let recovery = self
            .thread_recovery_path(&recovery_id)?
            .join("lent-routing");
        let home = self.config.parent().ok_or(FAILED)?;
        let _ = threads::transition(home, &recovery, "openai", &provider);
        self.record_thread_loans(&recovery, &provider, &provider, &recovery_id)
    }

    fn thread_recovery_path(&self, id: &str) -> Result<PathBuf, String> {
        if !valid_recovery_id(id) {
            return Err(CONFLICT.into());
        }
        Ok(self
            .receipt
            .parent()
            .ok_or(FAILED)?
            .join("codex-thread-recovery")
            .join(id))
    }

    fn loan_journal_path(&self) -> Result<PathBuf, String> {
        Ok(self
            .receipt
            .parent()
            .ok_or(FAILED)?
            .join("codex-thread-loans.json"))
    }

    fn load_thread_loans(&self) -> Result<LoanJournal, String> {
        let path = self.loan_journal_path()?;
        if fs::symlink_metadata(&path)
            .is_ok_and(|meta| !meta.is_file() || meta.file_type().is_symlink())
        {
            return Err(CONFLICT.into());
        }
        let Some(text) = read(&path)? else {
            return Ok(LoanJournal {
                version: 1,
                config: self.config.clone(),
                loans: Vec::new(),
            });
        };
        let journal: LoanJournal = serde_json::from_str(&text).map_err(|_| FAILED)?;
        if journal.version != 1
            || journal.config != self.config
            || journal
                .loans
                .iter()
                .any(|loan| !valid_recovery_id(&loan.recovery_id))
        {
            return Err(CONFLICT.into());
        }
        Ok(journal)
    }

    fn save_thread_loans(&self, journal: &LoanJournal) -> Result<(), String> {
        crate::config::atomic_write_private(
            &self.loan_journal_path()?,
            &serde_json::to_vec(journal).map_err(|_| FAILED)?,
        )
        .map_err(|_| FAILED.into())
    }

    fn record_thread_loans(
        &self,
        recovery: &Path,
        provider: &str,
        target: &str,
        recovery_id: &str,
    ) -> Result<(), String> {
        let ids = threads::recorded_ids(recovery, "openai", target)?;
        if ids.is_empty() {
            return Ok(());
        }
        let mut journal = self.load_thread_loans()?;
        if let Some(loan) = journal
            .loans
            .iter_mut()
            .find(|loan| loan.provider == provider && loan.recovery_id == recovery_id)
        {
            loan.ids.extend(ids);
        } else {
            journal.loans.push(ThreadLoan {
                provider: provider.to_owned(),
                recovery_id: recovery_id.to_owned(),
                ids,
            });
        }
        self.save_thread_loans(&journal)
    }

    fn recover_thread_loans(&self, active: &str) -> Result<bool, String> {
        let mut journal = self.load_thread_loans()?;
        if journal.loans.is_empty() {
            return Ok(false);
        }
        let home = self.config.parent().ok_or(FAILED)?;
        let mut retained = Vec::new();
        let mut incomplete = false;
        let mut changed = false;
        for mut loan in journal.loans {
            let recovery = self.thread_recovery_path(&loan.recovery_id)?;
            let before = loan.ids.len();
            for lender in [PROVIDER, loan.provider.as_str()] {
                loan.ids.extend(threads::recorded_ids(
                    &recovery.join("lent-routing"),
                    "openai",
                    lender,
                )?);
            }
            changed |= before != loan.ids.len();
            if active == PROVIDER || active == loan.provider {
                retained.push(loan);
                continue;
            }
            if active == "openai" {
                let recovery = self
                    .thread_recovery_path(&loan.recovery_id)?
                    .join("returned-loans");
                if pending(threads::transition_ids(
                    home,
                    &recovery,
                    &loan.provider,
                    "openai",
                    &loan.ids,
                )) {
                    incomplete = true;
                    retained.push(loan);
                    continue;
                }
            }
            // A switch to an unrelated provider never rewrites the loaned chats.
            changed = true;
        }
        journal.loans = retained;
        if changed {
            self.save_thread_loans(&journal)?;
        }
        Ok(incomplete)
    }

    fn save_thread_receipt(&self, receipt: &Receipt) -> Result<(), String> {
        crate::config::atomic_write_private(
            &self.receipt,
            &serde_json::to_vec(receipt).map_err(|_| FAILED)?,
        )
        .map_err(|_| FAILED.into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn migration_fixture(
        manager: &Manager,
        source: &str,
        target: &str,
        profiles: &str,
    ) -> Result<Receipt, Box<dyn std::error::Error>> {
        manager.write_projection(
            &[super::super::tests::group()],
            "fixture-local-token",
            "http://127.0.0.1:1234/intent/v1",
        )?;
        let mut receipt = manager.load()?.ok_or("fixture receipt")?;
        receipt.original = Some(format!("model_provider='{source}'\n{profiles}"));
        let mut installed: DocumentMut = receipt.installed.parse()?;
        installed["model_provider"] = toml_edit::value(target);
        receipt.installed = installed.to_string();
        receipt.thread_recovery_id = None;
        receipt.thread_sync_pending = false;
        fs::write(&manager.config, &receipt.installed)?;
        manager.save_thread_receipt(&receipt)?;
        Ok(receipt)
    }

    fn row_provider(dir: &Path, id: &str) -> Result<String, rusqlite::Error> {
        rusqlite::Connection::open(dir.join("state_5.sqlite"))?.query_row(
            "SELECT model_provider FROM threads WHERE id=?1",
            [id],
            |row| row.get(0),
        )
    }

    fn historical_table(id: &str) -> String {
        format!("[model_providers.{id}]\nname='VaryAI models and routing groups'\nbase_url='http://127.0.0.1:1234/v1'\nwire_api='responses'\nrequires_openai_auth=false\nexperimental_bearer_token='synthetic-historical-token'\nsupports_websockets=false\nsupports_standalone_web_search=false\n")
    }

    #[test]
    fn historical_origins_survive_repeated_native_and_standalone_modes(
    ) -> Result<(), Box<dyn std::error::Error>> {
        for paginated in [false, true] {
            let (dir, manager) = super::super::tests::setup();
            fs::write(
                &manager.config,
                format!(
                    "model='original'\n{}{}",
                    historical_table("historical_a"),
                    historical_table("historical_b")
                ),
            )?;
            for provider in ["historical_a", "historical_b"] {
                super::super::mode_tests::thread_model_fixture(
                    dir.path(),
                    provider,
                    provider,
                    paginated,
                    Some("unpublished-model"),
                    Some("different-old-model"),
                )?;
            }
            manager.write_projection(
                &[super::super::tests::group()],
                "fixture-local-token",
                "http://127.0.0.1:1234/intent/v1",
            )?;
            let mut receipt = manager.load()?.ok_or("fixture receipt")?;
            for target in ["openai", PROVIDER, "openai", PROVIDER] {
                let mut installed: DocumentMut = receipt.installed.parse()?;
                installed["model_provider"] = toml_edit::value(target);
                receipt.installed = installed.to_string();
                fs::write(&manager.config, &receipt.installed)?;
                manager.reconcile_thread_receipt(&mut receipt, false)?;
                for provider in ["historical_a", "historical_b"] {
                    assert_eq!(row_provider(dir.path(), provider)?, target);
                }
                assert!(!receipt.thread_sync_pending);
            }
            manager.reconcile_thread_receipt(&mut receipt, true)?;
            for provider in ["historical_a", "historical_b"] {
                assert_eq!(row_provider(dir.path(), provider)?, provider);
            }
        }
        Ok(())
    }

    #[test]
    fn historical_provider_table_edits_stop_new_automatic_moves(
    ) -> Result<(), Box<dyn std::error::Error>> {
        let (dir, manager) = super::super::tests::setup();
        fs::write(&manager.config, historical_table("historical"))?;
        manager.write_projection(
            &[super::super::tests::group()],
            "fixture-local-token",
            "http://127.0.0.1:1234/intent/v1",
        )?;
        let mut edited: DocumentMut = fs::read_to_string(&manager.config)?.parse()?;
        edited["model_providers"]["historical"]["base_url"] =
            toml_edit::value("https://synthetic.example/v1");
        fs::write(&manager.config, edited.to_string())?;
        let path = super::super::mode_tests::thread_model_fixture(
            dir.path(),
            "late-chat",
            "historical",
            false,
            None,
            None,
        )?;
        let before = fs::read(&path)?;
        manager.sync_threads()?;
        assert_eq!(row_provider(dir.path(), "late-chat")?, "historical");
        assert_eq!(fs::read(&path)?, before);
        Ok(())
    }

    #[test]
    fn native_provider_migration_only_restores_the_brought_chats(
    ) -> Result<(), Box<dyn std::error::Error>> {
        for paginated in [false, true] {
            let (dir, manager) = super::super::tests::setup();
            let mut receipt = migration_fixture(&manager, "previous-provider", "openai", "")?;
            super::super::mode_tests::thread_model_fixture(
                dir.path(),
                "previous-chat",
                "previous-provider",
                paginated,
                Some("fixture-prior-model"),
                Some("fixture-prior-model"),
            )?;
            let original = super::super::mode_tests::thread_model_fixture(
                dir.path(),
                "openai-chat",
                "openai",
                paginated,
                None,
                None,
            )?;
            let original_bytes = fs::read(&original)?;
            manager.reconcile_thread_receipt(&mut receipt, false)?;
            assert_eq!(row_provider(dir.path(), "previous-chat")?, "openai");
            assert_eq!(row_provider(dir.path(), "openai-chat")?, "openai");
            manager.reconcile_thread_receipt(&mut receipt, true)?;
            assert_eq!(
                row_provider(dir.path(), "previous-chat")?,
                "previous-provider"
            );
            assert_eq!(row_provider(dir.path(), "openai-chat")?, "openai");
            assert_eq!(fs::read(&original)?, original_bytes);
        }
        Ok(())
    }

    #[test]
    fn standalone_provider_borrows_openai_chats_and_returns_them_when_openai_is_selected(
    ) -> Result<(), Box<dyn std::error::Error>> {
        for paginated in [false, true] {
            let (dir, manager) = super::super::tests::setup();
            let mut receipt = migration_fixture(&manager, "previous-provider", PROVIDER, "")?;
            for (id, provider) in [
                ("previous-chat", "previous-provider"),
                ("openai-chat", "openai"),
                ("unrelated-chat", "unrelated"),
            ] {
                super::super::mode_tests::thread_model_fixture(
                    dir.path(),
                    id,
                    provider,
                    paginated,
                    None,
                    None,
                )?;
            }
            manager.reconcile_thread_receipt(&mut receipt, false)?;
            assert_eq!(row_provider(dir.path(), "previous-chat")?, PROVIDER);
            assert_eq!(row_provider(dir.path(), "openai-chat")?, PROVIDER);
            assert_eq!(row_provider(dir.path(), "unrelated-chat")?, "unrelated");
            manager.reconcile_thread_receipt(&mut receipt, true)?;
            assert_eq!(
                row_provider(dir.path(), "previous-chat")?,
                "previous-provider"
            );
            assert_eq!(
                row_provider(dir.path(), "openai-chat")?,
                "previous-provider"
            );
            fs::write(&manager.config, "model_provider='openai'\n")?;
            // Deferred restoration is independent from a connection receipt.
            fs::remove_file(&manager.receipt)?;
            manager.sync_threads()?;
            assert_eq!(
                row_provider(dir.path(), "previous-chat")?,
                "previous-provider"
            );
            assert_eq!(row_provider(dir.path(), "openai-chat")?, "openai");
            assert_eq!(row_provider(dir.path(), "unrelated-chat")?, "unrelated");
        }
        Ok(())
    }

    #[test]
    fn loans_follow_repeated_native_and_standalone_transitions(
    ) -> Result<(), Box<dyn std::error::Error>> {
        for paginated in [false, true] {
            let (dir, manager) = super::super::tests::setup();
            let mut receipt = migration_fixture(&manager, "previous-provider", PROVIDER, "")?;
            for (id, provider) in [
                ("previous-chat", "previous-provider"),
                ("openai-chat", "openai"),
            ] {
                super::super::mode_tests::thread_model_fixture(
                    dir.path(),
                    id,
                    provider,
                    paginated,
                    None,
                    None,
                )?;
            }
            manager.reconcile_thread_receipt(&mut receipt, false)?;
            for target in ["openai", PROVIDER, "openai", PROVIDER] {
                let mut installed: DocumentMut = receipt.installed.parse()?;
                installed["model_provider"] = toml_edit::value(target);
                receipt.installed = installed.to_string();
                fs::write(&manager.config, &receipt.installed)?;
                manager.reconcile_thread_receipt(&mut receipt, false)?;
                assert_eq!(row_provider(dir.path(), "previous-chat")?, target);
                assert_eq!(row_provider(dir.path(), "openai-chat")?, target);
                assert!(!receipt.thread_sync_pending);
            }
            manager.reconcile_thread_receipt(&mut receipt, true)?;
            assert_eq!(
                row_provider(dir.path(), "previous-chat")?,
                "previous-provider"
            );
            assert_eq!(
                row_provider(dir.path(), "openai-chat")?,
                "previous-provider"
            );
            fs::write(&manager.config, "model_provider='openai'\n")?;
            fs::remove_file(&manager.receipt)?;
            manager.sync_threads()?;
            assert_eq!(
                row_provider(dir.path(), "previous-chat")?,
                "previous-provider"
            );
            assert_eq!(row_provider(dir.path(), "openai-chat")?, "openai");
        }
        Ok(())
    }

    #[test]
    fn busy_source_chats_do_not_prevent_independent_openai_loans(
    ) -> Result<(), Box<dyn std::error::Error>> {
        let (dir, manager) = super::super::tests::setup();
        let mut receipt = migration_fixture(&manager, "previous-provider", PROVIDER, "")?;
        let path = super::super::mode_tests::thread_model_fixture(
            dir.path(),
            "busy-chat",
            "previous-provider",
            true,
            None,
            None,
        )?;
        fs::File::options()
            .write(true)
            .open(&path)?
            .set_times(fs::FileTimes::new().set_modified(std::time::SystemTime::now()))?;
        super::super::mode_tests::thread_model_fixture(
            dir.path(),
            "openai-chat",
            "openai",
            true,
            None,
            None,
        )?;
        manager.reconcile_thread_receipt(&mut receipt, false)?;
        assert!(receipt.thread_sync_pending);
        assert_eq!(row_provider(dir.path(), "busy-chat")?, "previous-provider");
        assert_eq!(row_provider(dir.path(), "openai-chat")?, PROVIDER);
        Ok(())
    }

    #[test]
    fn corrupt_loan_ledger_stays_pending_without_rewriting_history(
    ) -> Result<(), Box<dyn std::error::Error>> {
        let (dir, manager) = super::super::tests::setup();
        fs::write(&manager.config, "model_provider='openai'\n")?;
        let path = super::super::mode_tests::thread_model_fixture(
            dir.path(),
            "openai-chat",
            "openai",
            true,
            None,
            None,
        )?;
        let before = fs::read(&path)?;
        let journal = manager.loan_journal_path()?;
        fs::write(&journal, b"invalid synthetic journal")?;
        assert!(manager.thread_sync_pending());
        assert!(manager.sync_threads().is_err());
        assert_eq!(fs::read(&path)?, before);
        assert_eq!(row_provider(dir.path(), "openai-chat")?, "openai");
        assert_eq!(fs::read(&journal)?, b"invalid synthetic journal");
        assert!(!manager.receipt.exists());
        Ok(())
    }

    #[test]
    fn receiptless_loan_repayment_keeps_the_worker_pending_until_history_is_quiet(
    ) -> Result<(), Box<dyn std::error::Error>> {
        let (dir, manager) = super::super::tests::setup();
        let mut receipt = migration_fixture(&manager, "previous-provider", PROVIDER, "")?;
        let path = super::super::mode_tests::thread_model_fixture(
            dir.path(),
            "openai-chat",
            "openai",
            true,
            None,
            None,
        )?;
        manager.reconcile_thread_receipt(&mut receipt, false)?;
        manager.reconcile_thread_receipt(&mut receipt, true)?;
        fs::remove_file(&manager.receipt)?;
        fs::write(&manager.config, "model_provider='previous-provider'\n")?;
        assert!(
            manager.thread_sync_pending(),
            "a future OpenAI selection must still be observed"
        );
        manager.sync_threads()?;
        assert_eq!(
            row_provider(dir.path(), "openai-chat")?,
            "previous-provider"
        );
        fs::File::options()
            .write(true)
            .open(&path)?
            .set_times(fs::FileTimes::new().set_modified(std::time::SystemTime::now()))?;
        fs::write(&manager.config, "model_provider='openai'\n")?;
        manager.sync_threads()?;
        assert!(manager.thread_sync_pending());
        assert_eq!(
            row_provider(dir.path(), "openai-chat")?,
            "previous-provider"
        );
        fs::File::options()
            .write(true)
            .open(&path)?
            .set_times(fs::FileTimes::new().set_modified(
                std::time::SystemTime::now() - std::time::Duration::from_secs(120),
            ))?;
        manager.sync_threads()?;
        assert_eq!(row_provider(dir.path(), "openai-chat")?, "openai");
        assert!(!manager.thread_sync_pending());
        Ok(())
    }

    #[test]
    fn receiptless_lending_requires_the_active_menu_credential(
    ) -> Result<(), Box<dyn std::error::Error>> {
        for connected in [false, true] {
            for matching_credential in [false, true] {
                let (dir, manager) = super::super::tests::setup();
                let token = if matching_credential {
                    "fixture-menu-token"
                } else {
                    "fixture-other-token"
                };
                fs::write(&manager.config, format!("model_provider='previous-provider'\n[model_providers.previous-provider]\nname='VaryAI models and routing groups'\nbase_url='http://127.0.0.1:1234/intent/v1'\nwire_api='responses'\nexperimental_bearer_token='{token}'\n"))?;
                if connected {
                    manager
                        .runtime
                        .set_menu(Arc::new(crate::proxy::intent::Menu {
                            token: "fixture-menu-token".into(),
                            groups: Default::default(),
                            models: Default::default(),
                            default_model: None,
                            omitted_entries: 0,
                            context_settings: Default::default(),
                        }))?;
                }
                super::super::mode_tests::thread_model_fixture(
                    dir.path(),
                    "openai-chat",
                    "openai",
                    true,
                    None,
                    None,
                )?;
                manager.sync_threads()?;
                assert_eq!(
                    row_provider(dir.path(), "openai-chat")?,
                    if connected && matching_credential {
                        "previous-provider"
                    } else {
                        "openai"
                    }
                );
                if connected && matching_credential {
                    manager.runtime.clear_menu();
                    fs::write(&manager.config, "model_provider='openai'\n")?;
                    manager.sync_threads()?;
                    assert_eq!(row_provider(dir.path(), "openai-chat")?, "openai");
                }
            }
        }
        Ok(())
    }

    #[test]
    fn same_display_name_uses_provider_ids_for_openai_loans(
    ) -> Result<(), Box<dyn std::error::Error>> {
        let (dir, manager) = super::super::tests::setup();
        let mut receipt =
            migration_fixture(&manager, "previous-provider", "previous-provider", "")?;
        receipt.original = Some("model_provider='previous-provider'\n[model_providers.previous-provider]\nname='VaryAI models and routing groups'\n".to_owned());
        let mut installed: DocumentMut = receipt.installed.parse()?;
        installed["model_providers"]["previous-provider"] = toml_edit::table();
        installed["model_providers"]["previous-provider"]["name"] =
            toml_edit::value("VaryAI models and routing groups");
        receipt.installed = installed.to_string();
        fs::write(&manager.config, &receipt.installed)?;
        for (id, provider) in [
            ("old-openai-chat", "openai"),
            ("current-chat", "previous-provider"),
        ] {
            super::super::mode_tests::thread_model_fixture(
                dir.path(),
                id,
                provider,
                true,
                None,
                None,
            )?;
        }
        manager.reconcile_thread_receipt(&mut receipt, false)?;
        assert_eq!(
            row_provider(dir.path(), "old-openai-chat")?,
            "previous-provider"
        );
        assert_eq!(
            row_provider(dir.path(), "current-chat")?,
            "previous-provider"
        );
        fs::write(&manager.config, "model_provider='openai'\n")?;
        manager.sync_threads()?;
        assert_eq!(row_provider(dir.path(), "old-openai-chat")?, "openai");
        assert_eq!(
            row_provider(dir.path(), "current-chat")?,
            "previous-provider"
        );
        Ok(())
    }

    #[test]
    fn lending_respects_openai_profiles_and_external_provider_changes(
    ) -> Result<(), Box<dyn std::error::Error>> {
        for named_profile in [false, true] {
            let (dir, manager) = super::super::tests::setup();
            let profiles = if named_profile {
                "[profiles.personal]\nmodel_provider='openai'\n"
            } else {
                ""
            };
            let mut receipt = migration_fixture(&manager, "previous-provider", PROVIDER, profiles)?;
            super::super::mode_tests::thread_model_fixture(
                dir.path(),
                "openai-chat",
                "openai",
                true,
                None,
                None,
            )?;
            manager.reconcile_thread_receipt(&mut receipt, false)?;
            assert_eq!(
                row_provider(dir.path(), "openai-chat")?,
                if named_profile { "openai" } else { PROVIDER }
            );
            if !named_profile {
                manager.reconcile_thread_receipt(&mut receipt, true)?;
                let db = rusqlite::Connection::open(dir.path().join("state_5.sqlite"))?;
                db.execute("UPDATE threads SET model_provider='external'", [])?;
                fs::write(&manager.config, "model_provider='openai'\n")?;
                fs::remove_file(&manager.receipt)?;
                manager.sync_threads()?;
                assert_eq!(row_provider(dir.path(), "openai-chat")?, "external");
            }
        }
        Ok(())
    }

    #[test]
    fn provider_migration_preserves_unpublished_or_missing_saved_models(
    ) -> Result<(), Box<dyn std::error::Error>> {
        for paginated in [false, true] {
            for saved_model in [Some("codex/fixture-previous-model"), Some(""), None] {
                let (dir, manager) = super::super::tests::setup();
                fs::write(
                    &manager.config,
                    "model_provider='magpie'\nmodel='codex/fixture-previous-model'\n",
                )?;
                let path = super::super::mode_tests::thread_model_fixture(
                    dir.path(),
                    "previous-provider-chat",
                    "magpie",
                    paginated,
                    saved_model,
                    Some("fixture-previous-turn-model"),
                )?;
                let before = fs::read(&path)?;
                let modified = fs::metadata(&path)?.modified()?;
                let db = rusqlite::Connection::open(dir.path().join("state_5.sqlite"))?;
                manager.write_projection(
                    &[super::super::tests::group()],
                    "fixture-local-token",
                    "http://127.0.0.1:1234/intent/v1",
                )?;
                manager.sync_threads()?;
                let (provider, model): (String, Option<String>) = db.query_row(
                    "SELECT model_provider, model FROM threads WHERE id='previous-provider-chat'",
                    [],
                    |row| Ok((row.get(0)?, row.get(1)?)),
                )?;
                assert_eq!(
                    provider, PROVIDER,
                    "paginated={paginated}, model={saved_model:?}"
                );
                assert_eq!(model.as_deref(), saved_model);
                let expected = if paginated {
                    before.clone()
                } else {
                    String::from_utf8(before.clone())?
                        .replace(
                            "\"model_provider\":\"magpie\"",
                            "\"model_provider\":\"varyai\"",
                        )
                        .into_bytes()
                };
                assert_eq!(fs::read(&path)?, expected);
                assert_eq!(fs::metadata(&path)?.modified()?, modified);
                assert!(!manager.thread_sync_pending());
                manager.restore()?;
                let provider: String = db.query_row(
                    "SELECT model_provider FROM threads WHERE id='previous-provider-chat'",
                    [],
                    |row| row.get(0),
                )?;
                assert_eq!(provider, "magpie");
                assert_eq!(fs::read(&path)?, before);
            }
        }
        Ok(())
    }
}

fn historical_provider_ids(config: &toml::Value, source: &str) -> Vec<String> {
    config
        .get("model_providers")
        .and_then(toml::Value::as_table)
        .into_iter()
        .flat_map(|providers| providers.iter())
        .filter_map(|(id, value)| {
            // Validate before placing an ID inside the private recovery path.
            let safe_id = !id.is_empty()
                && id.len() <= 64
                && id
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-' | b'.'));
            if !safe_id
                || matches!(id.as_str(), PROVIDER | "openai")
                || id == source
                || profile_names_provider(config, id)
            {
                return None;
            }
            let table = value.as_table()?;
            (menu_provider_signature(table, "/v1") || menu_provider_signature(table, "/intent/v1"))
                .then(|| id.clone())
        })
        .collect()
}

fn effective_provider(config: &toml::Value) -> String {
    selected_profile_provider(config)
        .or_else(|| {
            config
                .get("model_provider")
                .and_then(toml::Value::as_str)
                .filter(|provider| !provider.is_empty())
        })
        .unwrap_or("openai")
        .to_owned()
}

fn source_provider(config: &toml::Value) -> String {
    let source = effective_provider(config);
    if selected_profile_provider(config).is_none() && source == PROVIDER {
        "openai".to_owned()
    } else {
        source
    }
}

fn selected_profile_provider(config: &toml::Value) -> Option<&str> {
    let profile = config.get("profile").and_then(toml::Value::as_str)?;
    if profile.is_empty() {
        return None;
    }
    config
        .get("profiles")
        .and_then(|profiles| profiles.get(profile))
        .and_then(|profile| profile.get("model_provider"))
        .and_then(toml::Value::as_str)
        .filter(|provider| !provider.is_empty())
}

fn profile_names_provider(config: &toml::Value, provider: &str) -> bool {
    config
        .get("profiles")
        .and_then(toml::Value::as_table)
        .is_some_and(|profiles| {
            profiles.values().any(|profile| {
                profile.get("model_provider").and_then(toml::Value::as_str) == Some(provider)
            })
        })
}
