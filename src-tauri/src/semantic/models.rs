//! Das Modell der Suche nach Bedeutung und seine Einbettung, plattformneutral:
//! IBM Granite Embedding 97M Multilingual R2 (Apache 2.0), ein kleiner
//! ModernBERT-Encoder, ausgeführt mit Candle auf der CPU.
//!
//! Revision und Prüfsummen stehen fest im Code: kommt mit einem TippIT-Update
//! eine neue Revision, lädt `semantic` sie beim nächsten Lauf nach. Eine eigene
//! Update-Abfrage gibt es nicht.

use std::path::Path;

use anyhow::{Context, Result};
use candle_core::{DType, Device, Tensor};
use candle_nn::VarBuilder;

use super::modernbert;

pub struct ModelFile {
    pub name: &'static str,
    pub size: u64,
    pub sha256: &'static str,
}

pub struct ModelSpec {
    pub repo: &'static str,
    /// Hugging-Face-Commit; die URL zeigt nie auf `main`.
    pub revision: &'static str,
    pub files: &'static [ModelFile],
    /// Gemessener RAM, solange das Modell geladen ist (Gewichte als f32 plus
    /// Tokenizer).
    pub ram_bytes: u64,
}

pub const MODEL: ModelSpec = ModelSpec {
    repo: "ibm-granite/granite-embedding-97m-multilingual-r2",
    revision: "835ad14087e140460703cf0fae09f97d469d65c2",
    files: &[
        ModelFile {
            name: "config.json",
            size: 1_216,
            sha256: "de948b0bdc6f356afad7a84b276d8dd7e7fe10fb9add1bb5e610621c28e41ebc",
        },
        ModelFile {
            name: "tokenizer.json",
            size: 25_301_672,
            sha256: "4f2842d568e2724370aec203652a42ac783c7937f8347a1a2cc7506d71f1582f",
        },
        ModelFile {
            name: "model.safetensors",
            size: 194_889_568,
            sha256: "f3ea88b230492811046145513710e76b4cc8c2ad49e8708da0e7247e548903be",
        },
    ],
    ram_bytes: 450_000_000,
};

impl ModelSpec {
    /// Kennung von Modell und Revision: Ordnername unter `models/` und
    /// `vectors.model`. Eine neue Revision ergibt neue Vektoren.
    pub fn id(&self) -> String {
        format!("granite-{}", &self.revision[..12])
    }

    pub fn url(&self, file: &ModelFile) -> String {
        format!(
            "https://huggingface.co/{}/resolve/{}/{}",
            self.repo, self.revision, file.name
        )
    }

    pub fn download_bytes(&self) -> u64 {
        self.files.iter().map(|f| f.size).sum()
    }
}

/// So viel Text je Eintrag bekommt das Modell: der Anfang trägt die
/// Bedeutung, und lange Logs kosteten sonst Sekunden je Eintrag.
const MAX_TOKENS: usize = 256;
const MAX_CHARS: usize = 4 * 1024;
/// Einbetten läuft auf eigenen zwei Threads: gemessen schneller als alle Kerne
/// (der Abstimmungsaufwand frisst den Gewinn bei kurzen Texten), und der
/// Rechner bleibt bedienbar.
const THREADS: usize = 2;

pub struct Embedder {
    model: modernbert::ModernBert,
    tokenizer: tokenizers::Tokenizer,
    pad: u32,
    pool: rayon::ThreadPool,
}

impl Embedder {
    pub fn load(dir: &Path) -> Result<Self> {
        let pool = rayon::ThreadPoolBuilder::new()
            .num_threads(THREADS)
            .thread_name(|i| format!("tippit-embed-{i}"))
            .build()?;
        let config: modernbert::Config =
            serde_json::from_slice(&std::fs::read(dir.join("config.json"))?)?;
        let weights = std::fs::read(dir.join("model.safetensors"))?;
        let vb = VarBuilder::from_buffered_safetensors(weights, DType::F32, &Device::Cpu)?;
        let model = modernbert::ModernBert::load(vb, &config, MAX_TOKENS)?;
        let mut tokenizer = tokenizers::Tokenizer::from_file(dir.join("tokenizer.json"))
            .map_err(anyhow::Error::msg)
            .context("tokenizer.json")?;
        // Gepolstert wird selbst (je Stapel auf die längste Eingabe).
        let pad = tokenizer.get_padding().map_or(0, |p| p.pad_id);
        tokenizer.with_padding(None);
        tokenizer
            .with_truncation(Some(tokenizers::TruncationParams {
                max_length: MAX_TOKENS,
                ..Default::default()
            }))
            .map_err(anyhow::Error::msg)?;
        Ok(Self {
            model,
            tokenizer,
            pad,
            pool,
        })
    }

    /// Normalisierte Vektoren, einer je Text: CLS-Token der letzten Schicht,
    /// wie im Beispiel der Modellkarte.
    pub fn embed(&self, texts: &[&str]) -> Result<Vec<Vec<f32>>> {
        let texts: Vec<String> = texts
            .iter()
            .map(|t| t.chars().take(MAX_CHARS).collect())
            .collect();
        let mut out = self.pool.install(|| self.forward(&texts))?;
        for v in &mut out {
            let norm = v.iter().map(|x| x * x).sum::<f32>().sqrt();
            if norm > 0.0 {
                v.iter_mut().for_each(|x| *x /= norm);
            }
        }
        Ok(out)
    }

    fn forward(&self, texts: &[String]) -> Result<Vec<Vec<f32>>> {
        let enc = self
            .tokenizer
            .encode_batch(texts.to_vec(), true)
            .map_err(anyhow::Error::msg)?;
        let len = enc
            .iter()
            .map(|e| e.get_ids().len())
            .max()
            .unwrap_or(1)
            .max(1);
        let mut ids = Vec::with_capacity(enc.len() * len);
        let mut mask = Vec::with_capacity(enc.len() * len);
        for e in &enc {
            let n = e.get_ids().len();
            ids.extend_from_slice(e.get_ids());
            ids.resize(ids.len() + len - n, self.pad);
            mask.resize(mask.len() + n, 1u32);
            mask.resize(mask.len() + len - n, 0u32);
        }
        let ids = Tensor::from_vec(ids, (enc.len(), len), &Device::Cpu)?;
        let mask = Tensor::from_vec(mask, (enc.len(), len), &Device::Cpu)?;
        let hidden = self.model.forward(&ids, &mask)?;
        Ok(hidden.narrow(1, 0, 1)?.squeeze(1)?.to_vec2()?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Mit echten Modelldateien (nicht in der CI): sinnvolle Nachbarn, und ein
    /// Text einzeln ergibt denselben Vektor wie im gepolsterten Stapel.
    /// `TIPPIT_MODELS=<Ordner mit dem Modellordner, benannt wie `MODEL.id()`>`.
    #[test]
    #[ignore = "braucht TIPPIT_MODELS mit heruntergeladenem Modell"]
    fn embeddings_find_meaning_and_ignore_padding() {
        let root = std::path::PathBuf::from(std::env::var("TIPPIT_MODELS").unwrap());
        let m = Embedder::load(&root.join(MODEL.id())).unwrap();
        let v = m
            .embed(&[
                "Kontonummer",
                "IBAN: DE89 3704 0044 0532 0130 00, BIC COBADEFFXXX",
                "Termin am Montag um 10 Uhr beim Zahnarzt",
            ])
            .unwrap();
        let cos = |a: &[f32], b: &[f32]| a.iter().zip(b).map(|(x, y)| x * y).sum::<f32>();
        assert!(cos(&v[0], &v[1]) > cos(&v[0], &v[2]));
        let one = m.embed(&["Kontonummer"]).unwrap();
        assert!(cos(&one[0], &v[0]) > 0.9999);
    }
}
