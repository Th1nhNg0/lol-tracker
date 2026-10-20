use crate::{config::{Llm, Secrets}, i18n::{Lang, Msg}, model::Match, presentation::plain, riot::bounded_body};
use anyhow::{Result, bail, ensure};
use reqwest::{Client, Url, header::HeaderValue};
use serde_json::{Value, json};
use std::time::Duration;

/// Optional commentary through any OpenAI-compatible Chat Completions endpoint.
pub struct Commentator { http: Client, url: Url, key: Option<HeaderValue>, model: String, timeout: Duration }
impl Commentator {
    pub fn optional(cfg: &Llm, secrets: &Secrets) -> Option<Self> {
        if !cfg.enabled { return None; }
        match Self::new(cfg, &secrets.llm) {
            Ok(client) => Some(client),
            Err(error) => { tracing::warn!(%error, "LLM configuration unusable; using deterministic commentary only"); None }
        }
    }
    fn new(cfg: &Llm, key: &str) -> Result<Self> {
        ensure!(!cfg.model.trim().is_empty(), "llm.model is empty");
        ensure!((1..=120).contains(&cfg.timeout_secs), "llm.timeout_secs must be 1..120");
        let base = cfg.base_url.trim().trim_end_matches('/');
        let url = Url::parse(&format!("{base}/chat/completions")).map_err(|_| anyhow::anyhow!("Invalid llm.base_url"))?;
        ensure!(url.username().is_empty() && url.password().is_none() && url.query().is_none() && url.fragment().is_none(), "llm.base_url must not contain credentials, query or fragment");
        // Plain HTTP only for a server on this machine (Ollama, llama.cpp, LM Studio...).
        let local = matches!(url.host_str(), Some("localhost" | "127.0.0.1" | "[::1]"));
        ensure!(url.scheme() == "https" || (url.scheme() == "http" && local), "llm.base_url must be HTTPS (plain HTTP is allowed only for localhost)");
        let key = if key.is_empty() {
            ensure!(local, "LLM_API_KEY is required for a remote provider");
            None
        } else {
            let mut value = HeaderValue::from_str(&format!("Bearer {key}")).map_err(|_| anyhow::anyhow!("Invalid LLM_API_KEY format"))?;
            value.set_sensitive(true);
            Some(value)
        };
        let timeout = Duration::from_secs(cfg.timeout_secs);
        let http = Client::builder().timeout(timeout).connect_timeout(Duration::from_secs(5))
            .redirect(reqwest::redirect::Policy::none()).build().map_err(|_| anyhow::anyhow!("Cannot create LLM HTTP client"))?;
        Ok(Self { http, url, key, model: cfg.model.trim().to_owned(), timeout })
    }
    pub async fn enhance(&self, m: &Match, lang: Lang) -> Result<String> {
        tokio::time::timeout(self.timeout, self.request(m, lang)).await.map_err(|_| anyhow::anyhow!("LLM deadline exceeded"))?
    }
    async fn request(&self, m: &Match, lang: Lang) -> Result<String> {
        let players: Vec<_> = m.participants.iter().filter(|p| p.tracked).collect();
        ensure!(!m.remake && !players.is_empty(), "No commentary needed");
        // Anonymised: numeric participant IDs and final statistics only.
        let rows: Vec<_> = players.iter().map(|p| json!({
            "player": p.id, "role": p.role, "team": p.team, "win": p.win,
            "kills": p.kills, "deaths": p.deaths, "assists": p.assists, "cs": p.cs,
            "damage": p.damage, "damage_taken": p.damage_taken, "gold": p.gold, "vision": p.vision,
        })).collect();
        let prompt = format!("Write one concise, light-hearted but respectful observation per player in {}, based only on the supplied final statistics. Never invent timeline events, blame, intent or causation. No names, links, mentions or Markdown. Respond ONLY with a JSON array: [{{\"player\":1,\"comment\":\"at most 140 characters\"}}]. Include every supplied player exactly once; player is the supplied numeric ID.", lang.english_name());
        let payload = json!({ "model": self.model, "messages": [
            {"role": "system", "content": prompt},
            {"role": "user", "content": json!({"duration_seconds": m.duration, "players": rows}).to_string()}
        ], "max_tokens": 1000, "stream": false });
        let mut request = self.http.post(self.url.clone()).json(&payload);
        if let Some(key) = &self.key { request = request.header("Authorization", key.clone()); }
        let response = request.send().await.map_err(|_| anyhow::anyhow!("LLM network request failed"))?;
        ensure!(response.status().is_success(), "LLM provider returned HTTP {}", response.status().as_u16());
        let response: Value = serde_json::from_slice(&bounded_body(response, 256 * 1024).await?).map_err(|_| anyhow::anyhow!("Invalid LLM response"))?;
        let choice = &response["choices"][0];
        if let Some(reason @ ("length" | "content_filter")) = choice["finish_reason"].as_str() { bail!("LLM response incomplete ({reason})"); }
        let text = choice["message"]["content"].as_str().ok_or_else(|| anyhow::anyhow!("LLM returned no text"))?;
        let comments = parse(text)?;
        let mut out = vec![lang.t(Msg::AiHeader).to_owned()];
        for player in players {
            let mut found = comments.iter().filter(|(id, _)| *id == player.id);
            let (Some((_, comment)), None) = (found.next(), found.next()) else { bail!("LLM returned a duplicate or missing player"); };
            out.push(format!("{} — {}", plain(&player.name), comment));
        }
        Ok(out.join("\n"))
    }
}

/// Accepts a bare JSON array, one wrapped in a Markdown fence or prose, and string or numeric IDs.
fn parse(text: &str) -> Result<Vec<(u64, String)>> {
    let (Some(start), Some(end)) = (text.find('['), text.rfind(']')) else { bail!("LLM reply has no JSON array"); };
    ensure!(start < end, "LLM reply has no JSON array");
    let rows: Vec<Value> = serde_json::from_str(&text[start..=end]).map_err(|_| anyhow::anyhow!("Invalid LLM commentary JSON"))?;
    rows.iter().map(|row| {
        let id = row["player"].as_u64().or_else(|| row["player"].as_str()?.trim().parse().ok()).ok_or_else(|| anyhow::anyhow!("LLM comment lacks a player ID"))?;
        let raw = row["comment"].as_str().unwrap_or("");
        ensure!(!raw.contains("://"), "LLM comment contains a link");
        Ok((id, sanitize(raw)?))
    }).collect()
}
/// No markup, mentions or line breaks; bounded length.
fn sanitize(text: &str) -> Result<String> {
    let clean: String = text.chars().map(|c| if c.is_control() { ' ' } else { c })
        .filter(|c| !matches!(c, '`' | '*' | '_' | '~' | '|' | '@' | '<' | '>' | '\\')).collect();
    let clean = clean.split_whitespace().collect::<Vec<_>>().join(" ");
    ensure!(!clean.is_empty(), "Empty LLM comment");
    Ok(if clean.chars().count() > 160 { format!("{}…", clean.chars().take(159).collect::<String>()) } else { clean })
}
