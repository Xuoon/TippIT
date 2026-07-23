use std::collections::BTreeMap;

use convex::{ConvexClient, FunctionResult, Value};

use super::protocol::SyncEntry;

/// Dünner Wrapper um den offiziellen Convex-Rust-Client.
pub struct SyncClient {
    client: ConvexClient,
    group_id: String,
    /// Hex-kodierter Auth-Key; der Server speichert nur sha256(hex-String).
    auth_key: String,
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

impl SyncClient {
    pub async fn connect(url: &str, group_id: &str, auth_key: &[u8; 32]) -> anyhow::Result<Self> {
        let client = ConvexClient::new(url)
            .await
            .map_err(|e| anyhow::anyhow!("Convex-Verbindung fehlgeschlagen: {e}"))?;
        Ok(Self {
            client,
            group_id: group_id.to_string(),
            auth_key: hex(auth_key),
        })
    }

    fn base_args(&self) -> BTreeMap<String, Value> {
        let mut args = BTreeMap::new();
        args.insert("groupId".into(), Value::String(self.group_id.clone()));
        args.insert("authKey".into(), Value::String(self.auth_key.clone()));
        args
    }

    pub async fn create_group(&mut self) -> anyhow::Result<()> {
        let result = self
            .client
            .mutation("sync:createGroup", self.base_args())
            .await?;
        expect_value(result).map(|_| ())
    }

    pub async fn push(&mut self, entries: &[SyncEntry]) -> anyhow::Result<i64> {
        let mut args = self.base_args();
        args.insert(
            "entries".into(),
            Value::Array(entries.iter().map(|e| e.to_value()).collect()),
        );
        let result = self.client.mutation("sync:push", args).await?;
        let value = expect_value(result)?;
        let Value::Object(m) = value else {
            anyhow::bail!("push: unerwartete Antwort");
        };
        Ok(m.get("maxLamport").and_then(as_i64).unwrap_or(0))
    }

    /// Pull ab seq-Cursor; eigene Einträge werden serverseitig gefiltert,
    /// der zurückgegebene max_seq läuft trotzdem über sie hinweg.
    pub async fn pull_since(
        &mut self,
        since: i64,
        exclude_device: &str,
    ) -> anyhow::Result<PullPage> {
        let mut args = self.base_args();
        args.insert("since".into(), Value::Float64(since as f64));
        args.insert(
            "excludeDevice".into(),
            Value::String(exclude_device.to_string()),
        );
        let result = self.client.query("sync:pullSince", args).await?;
        let value = expect_value(result)?;
        let Value::Object(m) = value else {
            anyhow::bail!("pullSince: unerwartete Antwort");
        };
        let entries = match m.get("entries") {
            // Nicht dekodierbare Einträge dürfen NIE still übersprungen werden:
            // der seq-Cursor liefe darüber hinweg und der Eintrag wäre auf diesem
            // Gerät dauerhaft verloren. Fehler → Session-Retry, Watermark bleibt.
            Some(Value::Array(items)) => items
                .iter()
                .map(|v| {
                    SyncEntry::from_value(v).ok_or_else(|| {
                        anyhow::anyhow!("pullSince: Eintrag nicht dekodierbar (Format-Drift?)")
                    })
                })
                .collect::<anyhow::Result<Vec<_>>>()?,
            _ => vec![],
        };
        Ok(PullPage {
            entries,
            max_seq: m.get("maxSeq").and_then(as_i64).unwrap_or(since),
            has_more: matches!(m.get("hasMore"), Some(Value::Boolean(true))),
        })
    }

    pub async fn subscribe_latest(&mut self) -> anyhow::Result<convex::QuerySubscription> {
        self.client
            .subscribe("sync:latestSeq", self.base_args())
            .await
    }

    /// Gerät in der Gruppen-Geräteliste anmelden bzw. „zuletzt aktiv" auffrischen.
    pub async fn announce_device(
        &mut self,
        device_id: &str,
        name: &str,
        platform: &str,
    ) -> anyhow::Result<()> {
        let mut args = self.base_args();
        args.insert("deviceId".into(), Value::String(device_id.into()));
        args.insert("name".into(), Value::String(name.into()));
        args.insert("platform".into(), Value::String(platform.into()));
        let result = self.client.mutation("sync:announceDevice", args).await?;
        expect_value(result).map(|_| ())
    }

    pub async fn list_devices(&mut self) -> anyhow::Result<Vec<RemoteDevice>> {
        let result = self
            .client
            .query("sync:listDevices", self.base_args())
            .await?;
        let value = expect_value(result)?;
        let Value::Array(items) = value else {
            anyhow::bail!("listDevices: unerwartete Antwort");
        };
        items
            .iter()
            .map(|item| {
                let Value::Object(m) = item else {
                    anyhow::bail!("listDevices: Gerät nicht dekodierbar");
                };
                let text = |key: &str| match m.get(key) {
                    Some(Value::String(s)) => Ok(s.clone()),
                    _ => Err(anyhow::anyhow!("listDevices: Feld {key} fehlt")),
                };
                Ok(RemoteDevice {
                    device_id: text("deviceId")?,
                    name: text("name")?,
                    platform: text("platform")?,
                    last_seen_at: m.get("lastSeenAt").and_then(as_i64).unwrap_or(0),
                })
            })
            .collect()
    }
}

pub struct RemoteDevice {
    pub device_id: String,
    pub name: String,
    pub platform: String,
    pub last_seen_at: i64,
}

pub struct PullPage {
    pub entries: Vec<SyncEntry>,
    pub max_seq: i64,
    pub has_more: bool,
}

fn as_i64(v: &Value) -> Option<i64> {
    match v {
        Value::Float64(f) => Some(*f as i64),
        Value::Int64(i) => Some(*i),
        _ => None,
    }
}

pub fn expect_value(result: FunctionResult) -> anyhow::Result<Value> {
    match result {
        FunctionResult::Value(v) => Ok(v),
        FunctionResult::ErrorMessage(msg) => anyhow::bail!("Convex-Fehler: {msg}"),
        FunctionResult::ConvexError(e) => anyhow::bail!("Convex-Fehler: {}", e.message),
    }
}

pub fn latest_from_result(result: &FunctionResult) -> anyhow::Result<i64> {
    match result {
        FunctionResult::Value(value) => {
            as_i64(value).ok_or_else(|| anyhow::anyhow!("latestSeq: unerwartete Antwort"))
        }
        FunctionResult::ErrorMessage(message) => anyhow::bail!("Convex-Fehler: {message}"),
        FunctionResult::ConvexError(error) => anyhow::bail!("Convex-Fehler: {}", error.message),
    }
}
