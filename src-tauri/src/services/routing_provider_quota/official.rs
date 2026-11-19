//! First-party account readers. Routing labels and model names never select a credential destination.
use super::{
    fetch_json, number, origin, reset, Allowance, Auth, QuotaTier, Status, Wallet, FAILED,
};
use crate::services::coding_plan::{parse_opencode_go_tiers, parse_zhipu_tiers};
use reqwest::Url;
use serde_json::Value;

#[derive(Clone, Copy)]
enum Reader {
    DeepSeek,
    Field(&'static str, &'static str),
    OpenRouter,
    AiHubMix,
    Novita,
    Command,
    Ollama,
    Glm,
    Kimi,
    MiniMax,
    OpenCode,
    Mistral,
    Unavailable,
}

pub(super) struct Source {
    pub(super) url: Url,
    reader: Reader,
}
impl Source {
    pub(super) fn resolve(base: &str, coding: bool) -> Option<Self> {
        let root = origin(base).ok()?;
        if root.scheme() != "https" || root.port().is_some() {
            return None;
        }
        let base = Url::parse(base).ok()?;
        let path = base.path().trim_end_matches('/');
        let host = root.host_str()?;
        let (target, reader) = match host {
            "api.deepseek.com" => ("/user/balance", Reader::DeepSeek),
            "api.moonshot.cn" | "api.moonshot.ai" => (
                "/v1/users/me/balance",
                Reader::Field(
                    "/data/available_balance",
                    if host.ends_with(".cn") { "CNY" } else { "USD" },
                ),
            ),
            "api.stepfun.com" | "api.stepfun.ai" => (
                "/v1/accounts",
                Reader::Field(
                    "/balance",
                    if host.ends_with(".com") { "CNY" } else { "USD" },
                ),
            ),
            "openrouter.ai" => ("/api/v1/credits", Reader::OpenRouter),
            "aihubmix.com" => ("/dashboard/billing/remain", Reader::AiHubMix),
            "api-inference.modelscope.cn" => (
                "https://modelscope.cn/openapi/v1/magicubes/balance",
                Reader::Field("/data/available_balance", "magicubes"),
            ),
            "open.bigmodel.cn" | "api.z.ai" if coding || path.starts_with("/api/coding/") => {
                ("/api/monitor/usage/quota/limit", Reader::Glm)
            }
            "open.bigmodel.cn" | "api.z.ai" => (
                "/api/biz/account/query-customer-account-report",
                Reader::Field(
                    "/data/availableBalance",
                    if host.ends_with(".cn") { "CNY" } else { "USD" },
                ),
            ),
            "api.kimi.com" | "api.kimi.ai" if path == "/coding" || path.starts_with("/coding/") => {
                ("/coding/v1/usages", Reader::Kimi)
            }
            "api.minimaxi.com" | "api.minimax.cn" | "api.minimax.io" => {
                ("/v1/token_plan/remains", Reader::MiniMax)
            }
            "api.novita.ai" => ("/v3/user/balance", Reader::Novita),
            "ollama.com" => ("/api/balance", Reader::Ollama),
            "api.commandcode.ai" => ("/alpha/billing/credits", Reader::Command),
            "api.mistral.ai" => ("/v1/billing/budget", Reader::Mistral),
            "opencode.ai" | "api.opencode.ai"
                if ["/zen/go", "/inference/go"].iter().any(|prefix| {
                    path.strip_prefix(prefix)
                        .is_some_and(|tail| tail.is_empty() || tail.starts_with('/'))
                }) =>
            {
                ("https://opencode.ai/zen/go/v1/usage", Reader::OpenCode)
            }
            "opencode.ai" | "api.opencode.ai" | "api.siliconflow.cn" | "api.siliconflow.com" => {
                ("/", Reader::Unavailable)
            }
            _ => return None,
        };
        Some(Self {
            url: root.join(target).ok()?,
            reader,
        })
    }

    pub(super) async fn read(&self, key: &str) -> Result<Allowance, String> {
        let auth = match self.reader {
            Reader::Unavailable => return Ok(Allowance::empty(Status::Unsupported)),
            Reader::Glm => Auth::Raw,
            Reader::OpenCode => Auth::OpenCode,
            _ => Auth::Bearer,
        };
        let mut budget = None;
        if matches!(self.reader, Reader::Mistral) {
            let url = Url::parse("https://console.mistral.ai/api/vibe/whoami")
                .map_err(|_| FAILED.to_owned())?;
            budget = match fetch_json(url, key, Auth::Bearer).await? {
                Ok(body) => match body["plan_type"]
                    .as_str()
                    .map(str::to_ascii_lowercase)
                    .as_deref()
                {
                    Some("api") => Some("api_budget"),
                    Some("chat") => Some("vibe_budget"),
                    _ => return Ok(Allowance::empty(Status::Unsupported)),
                },
                Err(status) => return Ok(Allowance::empty(status)),
            };
        }
        match fetch_json(self.url.clone(), key, auth).await? {
            Ok(body) => Ok(if let Some(field) = budget {
                mistral_budget(&body[field])
            } else {
                self.parse(&body)
            }),
            Err(status) => Ok(Allowance::empty(status)),
        }
    }

    pub(super) fn parse(&self, body: &Value) -> Allowance {
        let mut q = Allowance::empty(Status::Ready);
        if matches!(self.url.host_str(), Some("open.bigmodel.cn" | "api.z.ai"))
            && number(body.get("code").or(body.pointer("/error/code")))
                .is_some_and(|code| (1000.0..=1005.0).contains(&code))
        {
            return Allowance::empty(Status::Unauthorized);
        }
        if matches!(self.reader, Reader::MiniMax)
            && body
                .pointer("/base_resp/status_code")
                .and_then(Value::as_i64)
                == Some(1004)
        {
            return Allowance::empty(Status::Unauthorized);
        }
        if body["success"] == false
            || body["status"] == false
            || !body["error"].is_null()
            || body
                .pointer("/base_resp/status_code")
                .is_some_and(|v| v != 0)
        {
            return Allowance::empty(Status::Unsupported);
        }
        let n = |path| number(body.pointer(path));
        match self.reader {
            Reader::DeepSeek => {
                for row in body["balance_infos"].as_array().into_iter().flatten() {
                    if let Some(unit) = row["currency"]
                        .as_str()
                        .filter(|s| s.len() == 3 && s.bytes().all(|b| b.is_ascii_uppercase()))
                    {
                        q.wallet(unit, number(row.get("total_balance")));
                    }
                }
                q.balances.sort_by(|a, b| a.unit.cmp(&b.unit));
            }
            Reader::Field(path, unit) => q.wallet(unit, n(path)),
            Reader::OpenRouter => q.wallet(
                "USD",
                n("/data/total_credits")
                    .zip(n("/data/total_usage"))
                    .map(|(credits, used)| credits - used),
            ),
            Reader::Novita => q.wallet("USD", n("/availableBalance").map(|v| v / 10_000.0)),
            Reader::AiHubMix => q.wallet("USD", n("/total_usage").filter(|v| *v >= 0.0)),
            Reader::Ollama => {
                q.wallet("USD", n("/purchased/balance_usd"));
                for (field, name) in [("session", "session"), ("weekly", "weekly_limit")] {
                    let w = &body["included"][field];
                    q.tier(
                        name,
                        number(w.get("remaining_percent")).map(|v| 100.0 - v),
                        w.get("resets_at"),
                    );
                }
                q.tier(
                    "allowance",
                    percent_left(n("/included/balance_usd"), n("/included/allowance_usd")),
                    body.pointer("/included/period/until"),
                );
            }
            Reader::Command => command(body, &mut q),
            Reader::Glm => q.tiers = parse_zhipu_tiers(&body["data"], true),
            Reader::OpenCode => q.tiers = parse_opencode_go_tiers(body),
            Reader::MiniMax => minimax(body, &mut q),
            Reader::Kimi => kimi(body, &mut q),
            Reader::Mistral | Reader::Unavailable => (),
        }
        q.finish()
    }
}

fn percent_left(left: Option<f64>, limit: Option<f64>) -> Option<f64> {
    Some((1.0 - left? / limit.filter(|v| *v > 0.0)?) * 100.0)
}
impl Allowance {
    fn wallet(&mut self, unit: &str, amount: Option<f64>) {
        if let Some(amount) = amount.filter(|v| v.is_finite()) {
            self.balances.push(Wallet {
                unit: unit.into(),
                amount,
            });
        }
    }
    fn tier(&mut self, name: &str, percent: Option<f64>, at: Option<&Value>) {
        if let Some(used) = percent.filter(|v| v.is_finite()) {
            let resets_at = reset(at).or_else(|| {
                let epoch = number(at)?;
                if epoch <= 0.0 {
                    return None;
                }
                chrono::DateTime::from_timestamp_millis(if epoch < 1e11 {
                    (epoch * 1000.0) as i64
                } else {
                    epoch as i64
                })
                .map(|t| t.to_rfc3339())
            });
            self.tiers.push(QuotaTier {
                name: name.into(),
                utilization: used.clamp(0.0, 100.0),
                resets_at,
                used_value_usd: None,
                max_value_usd: None,
            });
        }
    }
    fn finish(mut self) -> Self {
        self.tiers.retain(|tier| tier.utilization.is_finite());
        if self.balances.is_empty() && self.tiers.is_empty() {
            self.status = Status::Unsupported;
        }
        self
    }
}
fn minimax(body: &Value, q: &mut Allowance) {
    let Some(item) = body["model_remains"]
        .as_array()
        .and_then(|rows| rows.iter().find(|r| r["model_name"] == "general"))
    else {
        return;
    };
    for (name, status, percent, end) in [
        (
            "five_hour",
            "current_interval_status",
            "current_interval_remaining_percent",
            "end_time",
        ),
        (
            "weekly_limit",
            "current_weekly_status",
            "current_weekly_remaining_percent",
            "weekly_end_time",
        ),
    ] {
        let used = match item[status].as_i64() {
            Some(2) => Some(100.0),
            Some(3) => None,
            None if name == "weekly_limit" => None,
            _ => number(item.get(percent)).map(|left| 100.0 - left),
        };
        q.tier(name, used, item.get(end));
    }
}
fn mistral_budget(budget: &Value) -> Allowance {
    let mut q = Allowance::empty(Status::Ready);
    if number(budget.get("initial_budget")).is_some_and(|v| v > 0.0) {
        q.tier(
            "monthly",
            number(budget.get("usage_percentage")),
            budget.get("reset_at"),
        );
    }
    q.finish()
}
fn command(body: &Value, q: &mut Allowance) {
    let c = &body["credits"];
    let plan = c["planId"]
        .as_str()
        .unwrap_or_default()
        .to_ascii_lowercase()
        .replace('_', "-");
    let monthly = number(c.get("monthlyCredits"));
    let mut dollars = vec![
        number(c.get("purchasedCredits")),
        number(c.get("freeCredits")),
    ];
    if ["individual-pro", "individual-go"]
        .iter()
        .any(|s| plan.starts_with(s))
        && !plan.starts_with("individual-provider")
    {
        q.wallet("credits", monthly);
    } else {
        dollars.push(monthly);
    }
    q.wallet("USD", dollars.into_iter().flatten().reduce(|a, b| a + b));
    for (field, name) in [("fiveHour", "five_hour"), ("weekly", "weekly_limit")] {
        let w = &body["windowLimits"][field];
        q.tier(name, percent_used(w, "cap"), w.get("resetAt"));
    }
}
fn percent_used(row: &Value, cap: &str) -> Option<f64> {
    Some(number(row.get("used"))? / number(row.get(cap)).filter(|v| *v > 0.0)? * 100.0)
}
fn kimi(body: &Value, q: &mut Allowance) {
    for (name, fields) in [
        ("five_hour", &["limit_5h", "limit5h"][..]),
        ("weekly_limit", &["limit_7d", "limit7d"]),
        (
            "monthly",
            &[
                "limit_month_total",
                "limitMonthTotal",
                "limit_month_code",
                "limitMonthCode",
            ],
        ),
    ] {
        for field in fields {
            let row = &body["usages"][field];
            if let Some(ratio) = number(row.get("used_ratio").or(row.get("usedRatio"))) {
                q.tier(name, Some(ratio * 100.0), kimi_reset(row));
                break;
            }
        }
    }
    let mut counted = |name: &str, row: &Value| {
        if !q.tiers.iter().any(|w| w.name == name) {
            q.tier(
                name,
                percent_used(row, "limit").or_else(|| {
                    percent_left(number(row.get("remaining")), number(row.get("limit")))
                }),
                kimi_reset(row),
            );
        }
    };
    counted("weekly_limit", &body["usage"]);
    for w in body["limits"].as_array().into_iter().flatten() {
        let window = &w["window"];
        let duration = number(window.get("duration"));
        let name = match (
            window["timeUnit"].as_str().or(window["time_unit"].as_str()),
            duration,
        ) {
            (Some("TIME_UNIT_MINUTE"), Some(300.0)) | (Some("TIME_UNIT_HOUR"), Some(5.0)) => {
                "five_hour"
            }
            (Some("TIME_UNIT_DAY"), Some(7.0)) | (Some("TIME_UNIT_WEEK"), Some(1.0)) => {
                "weekly_limit"
            }
            _ => "allowance",
        };
        counted(name, &w["detail"]);
    }
}
fn kimi_reset(row: &Value) -> Option<&Value> {
    ["resetTime", "resetAt", "reset_time", "reset_at"]
        .iter()
        .find_map(|key| row.get(key))
}
