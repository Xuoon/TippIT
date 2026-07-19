use std::collections::BTreeMap;

use convex::Value;

use crate::storage::db::EntryRow;

/// Settings werden als Pseudo-Eintrag mit fester UUID und kind=3 gesynct.
pub const KIND_SETTINGS: u8 = 3;
pub const SETTINGS_UUID: &str = "00000000-0000-4000-8000-5e771465e771";

#[derive(Clone, Debug)]
pub struct SyncEntry {
    pub uuid: String,
    pub kind: u8,
    pub cipher: Option<Vec<u8>>,
    pub thumb: Option<Vec<u8>>,
    pub pinned: bool,
    pub created_at: i64,
    pub deleted: bool,
    pub lamport: i64,
    pub device_id: String,
}

impl SyncEntry {
    pub fn from_row(row: &EntryRow) -> Self {
        Self {
            uuid: row.uuid.clone(),
            kind: row.kind,
            cipher: row.cipher.clone(),
            thumb: row.thumb.clone(),
            pinned: row.pinned,
            created_at: row.created_at,
            deleted: row.deleted,
            lamport: row.lamport,
            device_id: row.device_id.clone(),
        }
    }

    pub fn to_value(&self) -> Value {
        let mut m = BTreeMap::new();
        m.insert("uuid".into(), Value::String(self.uuid.clone()));
        m.insert("kind".into(), Value::Float64(self.kind as f64));
        if let Some(c) = &self.cipher {
            m.insert("cipher".into(), Value::Bytes(c.clone()));
        }
        if let Some(t) = &self.thumb {
            m.insert("thumbCipher".into(), Value::Bytes(t.clone()));
        }
        m.insert("pinned".into(), Value::Boolean(self.pinned));
        m.insert("createdAt".into(), Value::Float64(self.created_at as f64));
        m.insert("deleted".into(), Value::Boolean(self.deleted));
        m.insert("lamport".into(), Value::Float64(self.lamport as f64));
        m.insert("deviceId".into(), Value::String(self.device_id.clone()));
        Value::Object(m)
    }

    pub fn from_value(value: &Value) -> Option<Self> {
        let Value::Object(m) = value else { return None };
        Some(Self {
            uuid: as_string(m.get("uuid")?)?,
            kind: as_f64(m.get("kind")?)? as u8,
            cipher: m.get("cipher").and_then(as_bytes),
            thumb: m.get("thumbCipher").and_then(as_bytes),
            pinned: as_bool(m.get("pinned")?)?,
            created_at: as_f64(m.get("createdAt")?)? as i64,
            deleted: as_bool(m.get("deleted")?)?,
            lamport: as_f64(m.get("lamport")?)? as i64,
            device_id: as_string(m.get("deviceId")?)?,
        })
    }
}

fn as_string(v: &Value) -> Option<String> {
    match v {
        Value::String(s) => Some(s.clone()),
        _ => None,
    }
}

fn as_f64(v: &Value) -> Option<f64> {
    match v {
        Value::Float64(f) => Some(*f),
        Value::Int64(i) => Some(*i as f64),
        _ => None,
    }
}

fn as_bool(v: &Value) -> Option<bool> {
    match v {
        Value::Boolean(b) => Some(*b),
        _ => None,
    }
}

fn as_bytes(v: &Value) -> Option<Vec<u8>> {
    match v {
        Value::Bytes(b) => Some(b.clone()),
        _ => None,
    }
}

/// LWW-Vergleich: (lamport, device_id) — deterministisch auf allen Geräten.
pub fn is_newer(lamport_a: i64, device_a: &str, lamport_b: i64, device_b: &str) -> bool {
    lamport_a > lamport_b || (lamport_a == lamport_b && device_a > device_b)
}
