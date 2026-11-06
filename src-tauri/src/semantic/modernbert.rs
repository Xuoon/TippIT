//! ModernBERT-Encoder für die Suche nach Bedeutung (Granite Embedding R2).
//!
//! Gekürzt übernommen aus `candle-transformers` 0.11.1
//! (`src/models/modernbert.rs`, MIT OR Apache-2.0, © The HuggingFace Team).
//! Geändert: nur der Encoder ohne MLM-/Klassifikationsköpfe, Aktivierung aus
//! `hidden_activation` statt fest GeLU (Granite R2 nutzt SiLU), Tensor-Namen
//! ohne `model.`-Präfix (Sentence-Transformers-Export), Rotary-Tabellen nur
//! bis zur tatsächlich genutzten Länge.

use std::sync::Arc;

use candle_core::{DType, Device, Result, Tensor, D};
use candle_nn::{
    embedding, layer_norm_no_bias, linear_no_bias, ops::softmax, Embedding, LayerNorm, Linear,
    VarBuilder,
};
use serde::Deserialize;

#[derive(Debug, Clone, Deserialize)]
pub struct Config {
    pub vocab_size: usize,
    pub hidden_size: usize,
    pub num_hidden_layers: usize,
    pub num_attention_heads: usize,
    pub intermediate_size: usize,
    pub layer_norm_eps: f64,
    pub global_attn_every_n_layers: usize,
    pub global_rope_theta: f64,
    pub local_attention: usize,
    pub local_rope_theta: f64,
    #[serde(default)]
    pub hidden_activation: Activation,
}

#[derive(Debug, Clone, Copy, Default, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Activation {
    #[default]
    Gelu,
    Silu,
}

struct Rotary {
    sin: Tensor,
    cos: Tensor,
}

impl Rotary {
    fn new(config: &Config, theta: f64, max_len: usize, dev: &Device) -> Result<Self> {
        let dim = config.hidden_size / config.num_attention_heads;
        let inv_freq: Vec<f32> = (0..dim)
            .step_by(2)
            .map(|i| 1f32 / theta.powf(i as f64 / dim as f64) as f32)
            .collect();
        let n = inv_freq.len();
        let inv_freq = Tensor::from_vec(inv_freq, (1, n), dev)?;
        let t = Tensor::arange(0u32, max_len as u32, dev)?
            .to_dtype(DType::F32)?
            .reshape((max_len, 1))?;
        let freqs = t.matmul(&inv_freq)?;
        Ok(Self {
            sin: freqs.sin()?,
            cos: freqs.cos()?,
        })
    }

    fn apply(&self, q: &Tensor, k: &Tensor) -> Result<(Tensor, Tensor)> {
        let seq_len = q.dim(2)?;
        let cos = self.cos.narrow(0, 0, seq_len)?;
        let sin = self.sin.narrow(0, 0, seq_len)?;
        Ok((
            candle_nn::rotary_emb::rope(&q.contiguous()?, &cos, &sin)?,
            candle_nn::rotary_emb::rope(&k.contiguous()?, &cos, &sin)?,
        ))
    }
}

struct Attention {
    qkv: Linear,
    proj: Linear,
    heads: usize,
    head_size: usize,
    rotary: Arc<Rotary>,
}

impl Attention {
    fn load(vb: VarBuilder, config: &Config, rotary: Arc<Rotary>) -> Result<Self> {
        Ok(Self {
            qkv: linear_no_bias(config.hidden_size, config.hidden_size * 3, vb.pp("Wqkv"))?,
            proj: linear_no_bias(config.hidden_size, config.hidden_size, vb.pp("Wo"))?,
            heads: config.num_attention_heads,
            head_size: config.hidden_size / config.num_attention_heads,
            rotary,
        })
    }

    fn forward(&self, xs: &Tensor, mask: &Tensor) -> Result<Tensor> {
        let (b, seq_len, d) = xs.dims3()?;
        let qkv = xs
            .apply(&self.qkv)?
            .reshape((b, seq_len, 3, self.heads, self.head_size))?
            .permute((2, 0, 3, 1, 4))?;
        let q = qkv.get(0)?;
        let k = qkv.get(1)?;
        let v = qkv.get(2)?;
        let (q, k) = self.rotary.apply(&q, &k)?;
        let scale = (self.head_size as f64).powf(-0.5);
        let q = (q * scale)?;
        let att = q.matmul(&k.transpose(D::Minus2, D::Minus1)?)?;
        let att = att.broadcast_add(mask)?;
        let att = softmax(&att, D::Minus1)?;
        att.matmul(&v.contiguous()?)?
            .transpose(1, 2)?
            .reshape((b, seq_len, d))?
            .apply(&self.proj)
    }
}

struct Mlp {
    wi: Linear,
    wo: Linear,
    act: Activation,
}

impl Mlp {
    fn load(vb: VarBuilder, config: &Config) -> Result<Self> {
        Ok(Self {
            wi: linear_no_bias(
                config.hidden_size,
                config.intermediate_size * 2,
                vb.pp("Wi"),
            )?,
            wo: linear_no_bias(config.intermediate_size, config.hidden_size, vb.pp("Wo"))?,
            act: config.hidden_activation,
        })
    }

    fn forward(&self, xs: &Tensor) -> Result<Tensor> {
        let xs = xs.apply(&self.wi)?.chunk(2, D::Minus1)?;
        // GLU: Aktivierung auf die erste Hälfte, mal die zweite.
        let gate = match self.act {
            Activation::Gelu => xs[0].gelu_erf()?,
            Activation::Silu => candle_nn::ops::silu(&xs[0])?,
        };
        (gate * &xs[1])?.apply(&self.wo)
    }
}

struct Layer {
    attn: Attention,
    mlp: Mlp,
    attn_norm: Option<LayerNorm>,
    mlp_norm: LayerNorm,
    local: bool,
}

impl Layer {
    fn forward(&self, xs: &Tensor, global: &Tensor, local: &Tensor) -> Result<Tensor> {
        let normed = match &self.attn_norm {
            Some(norm) => xs.apply(norm)?,
            None => xs.clone(),
        };
        let mask = if self.local {
            global.broadcast_add(local)?
        } else {
            global.clone()
        };
        let xs = (self.attn.forward(&normed, &mask)? + xs)?;
        let mlp = self.mlp.forward(&xs.apply(&self.mlp_norm)?)?;
        xs + mlp
    }
}

const MASKED: f32 = -1e30;

pub struct ModernBert {
    embeddings: Embedding,
    norm: LayerNorm,
    layers: Vec<Layer>,
    final_norm: LayerNorm,
    local_window: usize,
}

impl ModernBert {
    /// `max_len`: längste Eingabe in Tokens, für die Rotary-Tabellen.
    pub fn load(vb: VarBuilder, config: &Config, max_len: usize) -> Result<Self> {
        let dev = vb.device().clone();
        let global = Arc::new(Rotary::new(
            config,
            config.global_rope_theta,
            max_len,
            &dev,
        )?);
        let local = Arc::new(Rotary::new(config, config.local_rope_theta, max_len, &dev)?);
        let layers = (0..config.num_hidden_layers)
            .map(|i| {
                let vb = vb.pp(format!("layers.{i}"));
                let is_local = i % config.global_attn_every_n_layers != 0;
                let rotary = if is_local {
                    local.clone()
                } else {
                    global.clone()
                };
                Ok(Layer {
                    attn: Attention::load(vb.pp("attn"), config, rotary)?,
                    mlp: Mlp::load(vb.pp("mlp"), config)?,
                    // Die erste Schicht hat keine Attention-Norm (Identity).
                    attn_norm: layer_norm_no_bias(
                        config.hidden_size,
                        config.layer_norm_eps,
                        vb.pp("attn_norm"),
                    )
                    .ok(),
                    mlp_norm: layer_norm_no_bias(
                        config.hidden_size,
                        config.layer_norm_eps,
                        vb.pp("mlp_norm"),
                    )?,
                    local: is_local,
                })
            })
            .collect::<Result<Vec<_>>>()?;
        Ok(Self {
            embeddings: embedding(
                config.vocab_size,
                config.hidden_size,
                vb.pp("embeddings.tok_embeddings"),
            )?,
            norm: layer_norm_no_bias(
                config.hidden_size,
                config.layer_norm_eps,
                vb.pp("embeddings.norm"),
            )?,
            layers,
            final_norm: layer_norm_no_bias(
                config.hidden_size,
                config.layer_norm_eps,
                vb.pp("final_norm"),
            )?,
            local_window: config.local_attention / 2,
        })
    }

    /// `ids` und `mask` (1 = Token, 0 = Polster) als (Batch, Länge), u32.
    /// Liefert die letzten verborgenen Zustände (Batch, Länge, Breite).
    pub fn forward(&self, ids: &Tensor, mask: &Tensor) -> Result<Tensor> {
        let (b, seq_len) = ids.dims2()?;
        let dev = ids.device();
        // Endlich statt −∞: auch eine ganz maskierte Polster-Zeile bleibt so
        // NaN-frei, und NaN aus Polster-Zeilen erreichte über 0 · NaN die echten.
        let global = ((1.0 - mask.to_dtype(DType::F32)?)? * f64::from(MASKED))?
            .reshape((b, 1, 1, seq_len))?;
        let window: Vec<f32> = (0..seq_len)
            .flat_map(|i| {
                (0..seq_len).map(move |j| {
                    if i.abs_diff(j) > self.local_window {
                        MASKED
                    } else {
                        0.
                    }
                })
            })
            .collect();
        let local = Tensor::from_vec(window, (seq_len, seq_len), dev)?;
        let mut xs = ids.apply(&self.embeddings)?.apply(&self.norm)?;
        for layer in &self.layers {
            xs = layer.forward(&xs, &global, &local)?;
        }
        xs.apply(&self.final_norm)
    }
}
