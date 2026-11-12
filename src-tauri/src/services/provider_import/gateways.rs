//! Identify generated local gateway entries before building import candidates.
use super::*;

pub(super) struct Gateways {
    ports: HashSet<u16>,
}

impl Default for Gateways {
    fn default() -> Self {
        Self {
            ports: [3425, 15721, crate::proxy::types::DEFAULT_PROXY_PORT]
                .into_iter()
                .collect(),
        }
    }
}

fn ports(conn: &rusqlite::Connection) -> rusqlite::Result<Vec<u16>> {
    let mut stmt = conn.prepare("SELECT DISTINCT listen_port FROM proxy_config LIMIT 512")?;
    let rows = stmt
        .query_map([], |row| row.get::<_, i64>(0))?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(rows
        .into_iter()
        .filter_map(|port| u16::try_from(port).ok().filter(|port| *port != 0))
        .collect())
}

pub(super) fn generated_id(id: &str) -> bool {
    matches!(
        id.to_ascii_lowercase().as_str(),
        "magpie" | "cc-switch" | "cc_switch" | "ccswitch" | "varyai"
    )
}

fn loopback(url: &url::Url) -> bool {
    match url.host() {
        Some(url::Host::Domain(host)) => {
            host.trim_end_matches('.').eq_ignore_ascii_case("localhost")
        }
        Some(url::Host::Ipv4(ip)) => ip.is_loopback(),
        Some(url::Host::Ipv6(ip)) => {
            ip.is_loopback() || ip.to_ipv4_mapped().is_some_and(|ip| ip.is_loopback())
        }
        None => false,
    }
}

impl Gateways {
    pub(super) fn load(db: &Database, paths: &ImportPaths) -> Result<Self, String> {
        let mut gateways = Self::default();
        {
            let conn = db.conn.lock().map_err(|_| FAILED)?;
            gateways.ports.extend(ports(&conn).map_err(|_| FAILED)?);
        }
        // Release the application database before reading external configuration.
        if let Ok(settings) = json_file(&paths.provider_backup.with_file_name("settings.json")) {
            if let Some(port) = settings["port"]
                .as_u64()
                .and_then(|port| u16::try_from(port).ok())
                .filter(|port| *port >= 1024)
            {
                gateways.ports.insert(port);
            }
        }
        if let Ok(address) = std::env::var("MAGPIE_ADDR") {
            if let Ok(url) = url::Url::parse(&format!("http://{address}")) {
                if let Some(port) = url.port().filter(|port| *port != 0) {
                    gateways.ports.insert(port);
                }
            }
        }
        if paths.cc_switch.extension().is_some_and(|ext| ext == "db") {
            if let Ok(conn) = rusqlite::Connection::open_with_flags(
                &paths.cc_switch,
                rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY
                    | rusqlite::OpenFlags::SQLITE_OPEN_NO_MUTEX,
            ) {
                if conn.busy_timeout(std::time::Duration::from_secs(2)).is_ok() {
                    if let Ok(ports) = ports(&conn) {
                        gateways.ports.extend(ports);
                    }
                }
            }
        }
        Ok(gateways)
    }

    pub(super) fn matches_url(&self, raw: &str) -> bool {
        url::Url::parse(raw).is_ok_and(|url| {
            loopback(&url)
                && (url
                    .port_or_known_default()
                    .is_some_and(|port| self.ports.contains(&port))
                    || url.path().starts_with("/connections/")
                    || url.path().starts_with("/codex-menu")
                    || url.path().starts_with("/intent/")
                    || url.path() == "/backend-api/codex")
        })
    }

    pub(super) fn excludes(&self, candidate: &Candidate) -> bool {
        if !matches!(candidate.record.source, UpstreamSource::Api { .. }) {
            return false;
        }
        let generated = candidate.generated_gateway
            || keys(&candidate.record).iter().any(|key| {
                matches!(*key, "PROXY_MANAGED" | "magpie") || key.starts_with("magpie-")
            });
        protocol_endpoints(&candidate.record).values().any(|raw| {
            self.matches_url(raw)
                || (generated && url::Url::parse(raw).is_ok_and(|url| loopback(&url)))
        })
    }
}
