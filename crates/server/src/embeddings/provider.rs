use anyhow::{Result, anyhow, bail, ensure};
use reqwest::{Client, Url, header::HeaderValue};
use serde::Deserialize;
use sessiondb_types::EmbeddingProfile;
use std::{env, time::Duration};

// Bound individual UTF-8 inputs, including the transform, under the reference
// endpoint's 8192-token window. Long documents contribute every chunk.
const INPUT_BYTES: usize = 7900;

pub(crate) fn credential(base: &str) -> Option<HeaderValue> {
    let allowed = Url::parse(&env::var("SESSIONDB_EMBEDDING_BASE_URL").ok()?).ok()?;
    if allowed.as_str().trim_end_matches('/') != base {
        return None;
    }
    let secret = env::var("SESSIONDB_EMBEDDING_API_KEY").ok()?;
    if secret.trim().is_empty() {
        return None;
    }
    let mut value: HeaderValue = format!("Bearer {secret}").parse().ok()?;
    value.set_sensitive(true);
    Some(value)
}

pub(crate) struct EmbeddingClient(Client);

impl EmbeddingClient {
    pub(crate) fn new() -> Result<Self> {
        Ok(Self(
            Client::builder()
                .redirect(reqwest::redirect::Policy::none())
                .connect_timeout(Duration::from_secs(10))
                .build()?,
        ))
    }

    pub(crate) async fn query(&self, profile: &EmbeddingProfile, query: &str) -> Result<Vec<f32>> {
        let mut vectors = self.embed(profile, &[query], &profile.query_prefix).await?;
        Ok(vectors.remove(0))
    }

    pub(crate) async fn documents(
        &self,
        profile: &EmbeddingProfile,
        documents: &[&str],
    ) -> Result<Vec<Vec<f32>>> {
        self.embed(profile, documents, &profile.document_prefix)
            .await
    }

    async fn embed(
        &self,
        profile: &EmbeddingProfile,
        texts: &[&str],
        prefix: &str,
    ) -> Result<Vec<Vec<f32>>> {
        let mut parts = Vec::new();
        for (index, text) in texts.iter().enumerate() {
            let mut remaining = *text;
            while !remaining.is_empty() {
                let mut end = remaining.len().min(INPUT_BYTES - prefix.len());
                if end < remaining.len() {
                    end = remaining.floor_char_boundary(end);
                    let window = &remaining[..end];
                    if let Some((at, space)) =
                        window.char_indices().rfind(|(_, c)| c.is_whitespace())
                        && at >= end / 2
                    {
                        end = at + space.len_utf8();
                    }
                }
                let (part, rest) = remaining.split_at(end);
                parts.push((index, part));
                remaining = rest;
            }
        }
        let dims = profile.dimensions as usize;
        let mut sums = vec![vec![0.0_f64; dims]; texts.len()];
        let batch_size = (profile.batch_size as usize).min(1_000_000 / dims);
        tokio::time::timeout(Duration::from_secs(profile.timeout_seconds.into()), async {
            for batch in parts.chunks(batch_size) {
                let inputs: Vec<_> = batch
                    .iter()
                    .map(|(_, text)| format!("{prefix}{text}"))
                    .collect();
                let vectors = self.request(profile, &inputs).await?;
                for ((index, part), v) in batch.iter().zip(vectors) {
                    let weight = part.len() as f64 / norm(v.iter().map(|&x| f64::from(x)))?;
                    for (sum, value) in sums[*index].iter_mut().zip(v) {
                        *sum += f64::from(value) * weight;
                    }
                }
            }
            sums.into_iter()
                .map(|sum| {
                    let length = norm(sum.iter().copied())?;
                    Ok(sum.into_iter().map(|v| (v / length) as f32).collect())
                })
                .collect::<Result<Vec<_>>>()
        })
        .await
        .map_err(|_| anyhow!("embedding operation timed out"))?
    }

    async fn request(
        &self,
        profile: &EmbeddingProfile,
        inputs: &[String],
    ) -> Result<Vec<Vec<f32>>> {
        let mut request = self.0.post(format!("{}/embeddings",profile.base_url))
            .json(&serde_json::json!({"model":profile.model,"input":inputs,"encoding_format":"float"}));
        if profile.credential_configured {
            request = request.header(
                reqwest::header::AUTHORIZATION,
                credential(&profile.base_url)
                    .ok_or_else(|| anyhow!("embedding credential unavailable for this endpoint"))?,
            );
        }
        let mut response = request.send().await.map_err(|e| e.without_url())?;
        ensure!(
            response.status().is_success(),
            "embedding provider returned HTTP {}",
            response.status()
        );
        let mut bytes = Vec::new();
        let max = 65536 + inputs.len() * profile.dimensions as usize * 32;
        while let Some(chunk) = response.chunk().await.map_err(|e| e.without_url())? {
            ensure!(
                chunk.len() <= max - bytes.len(),
                "embedding response too large"
            );
            bytes.extend_from_slice(&chunk);
        }
        let result: ProviderResponse =
            serde_json::from_slice(&bytes).map_err(|_| anyhow!("invalid embedding response"))?;
        ensure!(
            result.model == profile.model && result.data.len() == inputs.len(),
            "unexpected embedding model or batch size"
        );
        let mut ordered = vec![None; inputs.len()];
        for entry in result.data {
            ensure!(
                entry.embedding.len() == profile.dimensions as usize,
                "unexpected embedding dimensions"
            );
            let slot = ordered
                .get_mut(entry.index)
                .ok_or_else(|| anyhow!("invalid embedding index"))?;
            ensure!(
                slot.replace(entry.embedding).is_none(),
                "duplicate embedding index"
            );
        }
        ordered
            .into_iter()
            .map(|v| v.ok_or_else(|| anyhow!("missing embedding index")))
            .collect()
    }
}

fn norm(values: impl Iterator<Item = f64>) -> Result<f64> {
    let length = values.map(|v| v * v).sum::<f64>().sqrt();
    if !length.is_finite() || length == 0.0 {
        bail!("invalid embedding norm");
    }
    Ok(length)
}

#[derive(Deserialize)]
struct ProviderResponse {
    model: String,
    data: Vec<Entry>,
}
#[derive(Deserialize)]
struct Entry {
    index: usize,
    embedding: Vec<f32>,
}
