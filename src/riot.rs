use crate::{config, model::{Player, Rank, n, s}};
use anyhow::{Result, bail, ensure};
use reqwest::{Client, Response, Url, header::{HeaderMap, HeaderValue}};
use serde_json::Value;
use std::{collections::HashMap, sync::Arc, time::Duration};
use tokio::{sync::Mutex, time::{Instant, sleep}};

#[derive(Clone)]
pub struct Riot {
    http: Client,
    headers: HeaderMap,
    pub platform: String,
    region: String,
    gate: Arc<Mutex<Instant>>,
    spacing: Duration,
}
impl Riot {
    pub fn new(key: &str, platform: &str, spacing_ms: u64) -> Result<Self> {
        let mut headers = HeaderMap::new();
        let mut token = HeaderValue::from_str(key).map_err(|_| anyhow::anyhow!("Invalid RIOT_API_KEY format"))?;
        token.set_sensitive(true);
        headers.insert("X-Riot-Token", token);
        let http = Client::builder().timeout(Duration::from_secs(20)).connect_timeout(Duration::from_secs(8))
            .redirect(reqwest::redirect::Policy::none()).user_agent("lol-tracker-rust/0.1")
            .build().map_err(|_| anyhow::anyhow!("Cannot initialize Riot HTTP client"))?;
        Ok(Self { http, headers, platform: platform.into(), region: config::region(platform).ok_or_else(|| anyhow::anyhow!("Invalid platform"))?.into(), gate: Arc::new(Mutex::new(Instant::now())), spacing: Duration::from_millis(spacing_ms) })
    }
    async fn get(&self, host: &str, segments: &[&str], query: &[(&str, String)], missing_ok: bool) -> Result<Option<Value>> {
        let mut url = Url::parse(&format!("https://{host}.api.riotgames.com")).map_err(|_| anyhow::anyhow!("Invalid Riot host"))?;
        url.path_segments_mut().map_err(|_| anyhow::anyhow!("Invalid Riot endpoint"))?.extend(segments);
        url.query_pairs_mut().extend_pairs(query.iter().map(|(k, v)| (*k, v.as_str())));
        for attempt in 0..3 {
            {
                let mut next = self.gate.lock().await;
                let delay = next.saturating_duration_since(Instant::now());
                // Long 429/auth backoffs should not freeze every account's worker.
                ensure!(delay <= Duration::from_secs(5), "Riot backoff active; will retry on next sweep");
                sleep(delay).await;
                *next = Instant::now() + self.spacing;
            }
            let response = self.http.get(url.clone()).headers(self.headers.clone()).send().await;
            let response = match response {
                Ok(r) => r,
                Err(_) if attempt < 2 => { sleep(Duration::from_secs(1 << attempt)).await; continue; }
                Err(_) => bail!("Riot network request failed"),
            };
            let status = response.status();
            if status.as_u16() == 404 && missing_ok { return Ok(None); }
            if status.as_u16() == 429 {
                let seconds = response.headers().get("Retry-After").and_then(|h| h.to_str().ok()).and_then(|h| h.parse::<f64>().ok()).filter(|n| n.is_finite() && *n >= 0.0).unwrap_or(5.0).ceil().min(86400.0) as u64;
                *self.gate.lock().await = Instant::now() + Duration::from_secs(seconds + 1);
                bail!("Riot rate limited the client; backing off");
            }
            if matches!(status.as_u16(), 401 | 403) {
                *self.gate.lock().await = Instant::now() + Duration::from_secs(60);
                bail!("Riot rejected credentials (401/403); development keys may have expired");
            }
            if status.is_server_error() && attempt < 2 { sleep(Duration::from_secs(1 << attempt)).await; continue; }
            ensure!(status.is_success(), "Riot request failed with HTTP {}", status.as_u16());
            let bytes = bounded_body(response, 16 * 1024 * 1024).await?;
            return serde_json::from_slice(&bytes).map(Some).map_err(|_| anyhow::anyhow!("Invalid Riot JSON response"));
        }
        bail!("Riot request retry limit reached")
    }
    pub async fn resolve(&self, player: &Player) -> Result<Option<String>> {
        let host = if self.region == "sea" { "asia" } else { &self.region };
        let v = self.get(host, &["riot", "account", "v1", "accounts", "by-riot-id", &player.game_name, &player.tag_line], &[], true).await?;
        Ok(v.and_then(|v| v["puuid"].as_str().filter(|s| !s.is_empty()).map(str::to_owned)))
    }
    pub async fn history(&self, puuid: &str, count: u8) -> Result<Vec<String>> {
        let value = self.get(&self.region, &["lol", "match", "v5", "matches", "by-puuid", puuid, "ids"], &[("count", count.to_string())], false).await?;
        serde_json::from_value(value.unwrap_or(Value::Null)).map_err(|_| anyhow::anyhow!("Invalid match history"))
    }
    /// Up to `count` match IDs that started at or after `start` (Unix seconds).
    pub async fn history_since(&self, puuid: &str, start: i64, count: u8) -> Result<Vec<String>> {
        let value = self.get(&self.region, &["lol", "match", "v5", "matches", "by-puuid", puuid, "ids"], &[("startTime", start.to_string()), ("count", count.to_string())], false).await?;
        serde_json::from_value(value.unwrap_or(Value::Null)).map_err(|_| anyhow::anyhow!("Invalid match history"))
    }
    pub async fn match_data(&self, id: &str) -> Result<Option<Value>> {
        self.get(&self.region, &["lol", "match", "v5", "matches", id], &[], true).await
    }
    pub async fn active(&self, puuid: &str) -> Result<Option<Value>> {
        self.get(&self.platform, &["lol", "spectator", "v5", "active-games", "by-summoner", puuid], &[], true).await
    }
    pub async fn ranks(&self, puuid: &str) -> Result<Vec<Rank>> {
        let value = self.get(&self.platform, &["lol", "league", "v4", "entries", "by-puuid", puuid], &[], false).await?;
        let rows = value.as_ref().and_then(Value::as_array).ok_or_else(|| anyhow::anyhow!("Invalid rank response"))?;
        Ok(rows.iter().filter_map(|v| {
            let queue = match s(v, "queueType").as_str() { "RANKED_SOLO_5x5" => 420, "RANKED_FLEX_SR" => 440, _ => return None };
            Some(Rank { queue, tier: s(v, "tier"), division: s(v, "rank"), lp: n(v, "leaguePoints") })
        }).collect())
    }
    // Presentation-only, no API key is sent to the CDN. Failure is non-fatal.
    pub async fn champions(&self) -> Result<HashMap<i64, String>> {
        let response = self.http.get("https://ddragon.leagueoflegends.com/api/versions.json").send().await.map_err(|_| anyhow::anyhow!("Data Dragon unavailable"))?;
        ensure!(response.status().is_success(), "Data Dragon unavailable");
        let versions: Vec<String> = serde_json::from_slice(&bounded_body(response, 256 * 1024).await?).map_err(|_| anyhow::anyhow!("Invalid Data Dragon versions"))?;
        let version = versions.first().ok_or_else(|| anyhow::anyhow!("No Data Dragon version"))?;
        ensure!(version.chars().all(|c| c.is_ascii_digit() || c == '.'), "Invalid Data Dragon version");
        let response = self.http.get(format!("https://ddragon.leagueoflegends.com/cdn/{version}/data/en_US/champion.json")).send().await.map_err(|_| anyhow::anyhow!("Data Dragon unavailable"))?;
        ensure!(response.status().is_success(), "Data Dragon unavailable");
        let v: Value = serde_json::from_slice(&bounded_body(response, 2 * 1024 * 1024).await?).map_err(|_| anyhow::anyhow!("Invalid champion catalog"))?;
        Ok(v["data"].as_object().into_iter().flat_map(|m| m.values()).filter_map(|c| Some((c["key"].as_str()?.parse().ok()?, c["name"].as_str()?.to_owned()))).collect())
    }
}
pub async fn bounded_body(mut response: Response, limit: usize) -> Result<Vec<u8>> {
    let mut bytes = Vec::new();
    while let Some(chunk) = response.chunk().await.map_err(|_| anyhow::anyhow!("HTTP response interrupted"))? {
        ensure!(bytes.len().saturating_add(chunk.len()) <= limit, "HTTP response exceeds size limit");
        bytes.extend_from_slice(&chunk);
    }
    Ok(bytes)
}
