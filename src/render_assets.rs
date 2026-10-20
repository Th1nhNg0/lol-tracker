//! Public Data Dragon artwork only. Never use authenticated Riot API headers here.
use crate::{model::Match, riot::bounded_body};
use anyhow::{Result, ensure};
use base64::{Engine, engine::general_purpose::STANDARD};
use reqwest::Client;
use serde_json::Value;
use std::{collections::{BTreeMap, BTreeSet, HashMap}, fs, path::{Path, PathBuf}, time::{Duration, SystemTime}};
use tokio::task::JoinSet;

/// Objective icon names on Community Dragon, one blue (100) and one red (200) file each.
pub const OBJECTIVES: [&str; 5] = ["dragon", "baron", "herald", "tower", "inhibitor"];

/// Ranked tiers that have an emblem on Community Dragon.
const TIERS: [&str; 10] = ["IRON", "BRONZE", "SILVER", "GOLD", "PLATINUM", "EMERALD", "DIAMOND", "MASTER", "GRANDMASTER", "CHALLENGER"];

/// Artwork an image needs, keyed by Match-V5 champion names and numeric IDs.
#[derive(Default)]
pub struct Wanted { pub champions: BTreeSet<String>, pub items: BTreeSet<i64>, pub objectives: bool, pub spells: BTreeSet<i64>, pub runes: BTreeSet<i64>, pub tiers: BTreeSet<String> }
impl Wanted {
    pub fn from_match(game: &Match) -> Self {
        Self {
            champions: game.participants.iter().map(|p| p.champion.clone()).collect(),
            items: game.participants.iter().flat_map(|p| p.items.iter().copied()).filter(|id| *id > 0).collect(),
            objectives: !game.objectives.is_empty(),
            tiers: game.participants.iter().filter_map(|p| p.rank_after.as_ref()).map(|r| r.tier.clone()).collect(),
            spells: game.participants.iter().flat_map(|p| p.spells.iter().copied()).collect(),
            runes: game.participants.iter().flat_map(|p| p.runes.iter().copied()).collect(),
        }
    }
}

#[derive(Default)]
pub struct Assets { pub images: HashMap<String, String>, pub names: HashMap<String, String> }
impl Assets {
    pub fn image(&self, key: &str) -> Option<&str> { self.images.get(key).map(String::as_str) }
    pub async fn load(wanted: Wanted, directory: PathBuf) -> Self {
        match tokio::time::timeout(Duration::from_secs(35), Self::prepare(wanted, directory)).await {
            Ok(Ok(assets)) => assets,
            _ => { tracing::warn!("Data Dragon artwork unavailable; using image placeholders"); Self::default() }
        }
    }
    async fn prepare(wanted: Wanted, directory: PathBuf) -> Result<Self> {
        fs::create_dir_all(&directory)?;
        let cache = Cache { directory, http: Client::builder().connect_timeout(Duration::from_secs(4)).timeout(Duration::from_secs(8))
            .redirect(reqwest::redirect::Policy::none()).user_agent("rift-recap/0.1").build()? };
        let versions = cache.json("/api/versions.json").await?;
        let version = versions[0].as_str().ok_or_else(|| anyhow::anyhow!("No Data Dragon version"))?.to_owned();
        ensure!(!version.is_empty() && version.chars().all(|c| c.is_ascii_digit() || c == '.'), "Invalid artwork version");
        let champions = cache.json(&format!("/cdn/{version}/data/en_US/champion.json")).await?;
        let mut jobs = BTreeMap::new();
        let mut names = HashMap::new();
        for wanted_name in &wanted.champions {
            let Some(champ) = champions["data"].as_object().and_then(|m| m.iter().find(|(id, c)| id.eq_ignore_ascii_case(wanted_name) || c["name"].as_str() == Some(wanted_name)).map(|(_, c)| c)) else { continue; };
            if let Some(name) = champ["name"].as_str() { names.insert(wanted_name.clone(), name.to_owned()); }
            if let Some(file) = champ["image"]["full"].as_str() { jobs.insert(format!("champion:{wanted_name}"), format!("/cdn/{version}/img/champion/{file}")); }
        }
        for id in &wanted.items { jobs.insert(format!("item:{id}"), format!("/cdn/{version}/img/item/{id}.png")); }
        if !wanted.spells.is_empty() {
            let spells = cache.json(&format!("/cdn/{version}/data/en_US/summoner.json")).await.unwrap_or(Value::Null);
            for id in &wanted.spells {
                if let Some(file) = spells["data"].as_object().and_then(|m| m.values().find(|v| v["key"].as_str().and_then(|s| s.parse::<i64>().ok()) == Some(*id))).and_then(|v| v["image"]["full"].as_str()) {
                    jobs.insert(format!("spell:{id}"), format!("/cdn/{version}/img/spell/{file}"));
                }
            }
        }
        if !wanted.runes.is_empty() {
            let runes = cache.json(&format!("/cdn/{version}/data/en_US/runesReforged.json")).await.unwrap_or(Value::Null);
            let mut rune_icons = HashMap::new();
            for style in runes.as_array().into_iter().flatten() {
                if let (Some(id), Some(icon)) = (style["id"].as_i64(), style["icon"].as_str()) { rune_icons.insert(id, icon.to_owned()); }
                for rune in style["slots"].as_array().into_iter().flatten().flat_map(|slot| slot["runes"].as_array().into_iter().flatten()) {
                    if let (Some(id), Some(icon)) = (rune["id"].as_i64(), rune["icon"].as_str()) { rune_icons.insert(id, icon.to_owned()); }
                }
            }
            for id in &wanted.runes {
                if let Some(icon) = rune_icons.get(id) { jobs.insert(format!("rune:{id}"), format!("/cdn/img/{icon}")); }
            }
        }
        for tier in &wanted.tiers {
            if TIERS.contains(&tier.as_str()) { jobs.insert(format!("tier:{tier}"), format!("/rank/emblem-{}.png", tier.to_lowercase())); }
        }
        if wanted.objectives {
            for kind in OBJECTIVES { for team in [100, 200] { jobs.insert(format!("objective:{kind}-{team}"), format!("/objective/{kind}-{team}.png")); } }
        }
        let mut images = HashMap::new();
        let mut running = JoinSet::new();
        for (key, path) in jobs {
            if running.len() >= 6
                && let Some(Ok(Some((key, image)))) = running.join_next().await { images.insert(key, image); }
            let cache = cache.clone();
            running.spawn(async move {
                let bytes = cache.bytes(&path, Duration::from_secs(30 * 86400)).await.ok()?;
                // Rank emblems are small shapes in a large transparent canvas; keep only a trimmed 64 px icon.
                let bytes = if key.starts_with("tier:") { emblem_icon(&bytes)? } else { bytes };
                let mime = if bytes.starts_with(b"\x89PNG\r\n\x1a\n") { "image/png" } else if bytes.starts_with(b"\xff\xd8") { "image/jpeg" } else { return None; };
                Some((key, format!("data:{mime};base64,{}", STANDARD.encode(bytes))))
            });
        }
        while let Some(result) = running.join_next().await { if let Ok(Some((key, image))) = result { images.insert(key, image); } }
        // Directory walk and deletions are blocking I/O; keep them off the async runtime.
        let directory = cache.directory.clone();
        tokio::task::spawn_blocking(move || prune(&directory));
        tracing::info!(images = images.len(), "Prepared public Data Dragon artwork");
        Ok(Self { images, names })
    }
}
#[derive(Clone)]
struct Cache { directory: PathBuf, http: Client }
impl Cache {
    async fn json(&self, path: &str) -> Result<Value> {
        serde_json::from_slice(&self.bytes(path, Duration::from_secs(86400)).await?).map_err(|_| anyhow::anyhow!("Invalid artwork metadata"))
    }
    async fn bytes(&self, path: &str, age: Duration) -> Result<Vec<u8>> {
        ensure!(path.len() <= 220 && (path.starts_with("/cdn/") || path.starts_with("/objective/") || path.starts_with("/rank/") || path == "/api/versions.json") && !path.split('/').any(|p| p == "..")
            && path.chars().all(|c| c.is_ascii_alphanumeric() || "/._-".contains(c)), "Invalid artwork path");
        let file = self.directory.join(path.replace('/', "_"));
        if let Ok(meta) = fs::metadata(&file)
            && meta.len() <= 2 * 1024 * 1024 && meta.modified().ok().and_then(|t| t.elapsed().ok()).is_some_and(|a| a < age)
                && let Ok(bytes) = fs::read(&file) { return Ok(bytes); }
        // Objective icons are the match-history assets of the League client, published by Community Dragon.
        let url = if let Some(name) = path.strip_prefix("/objective/") {
            format!("https://raw.communitydragon.org/latest/plugins/rcp-fe-lol-match-history/global/default/{name}")
        } else if let Some(name) = path.strip_prefix("/rank/") {
            format!("https://raw.communitydragon.org/latest/plugins/rcp-fe-lol-static-assets/global/default/images/ranked-emblem/{name}")
        } else { format!("https://ddragon.leagueoflegends.com{path}") };
        let response = self.http.get(url).send().await.map_err(|_| anyhow::anyhow!("Artwork network request failed"))?;
        ensure!(response.status().is_success(), "Artwork temporarily unavailable");
        let bytes = bounded_body(response, 2 * 1024 * 1024).await?;
        fs::write(file, &bytes)?;
        Ok(bytes)
    }
}
fn prune(directory: &Path) {
    let mut files = Vec::new();
    for entry in fs::read_dir(directory).into_iter().flatten().flatten() {
        if let Ok(meta) = entry.metadata() {
            if !meta.is_file() { continue; }
            let modified = meta.modified().unwrap_or(SystemTime::UNIX_EPOCH);
            if modified.elapsed().is_ok_and(|age| age > Duration::from_secs(30 * 86400)) { let _ = fs::remove_file(entry.path()); }
            else { files.push((modified, meta.len(), entry.path())); }
        }
    }
    files.sort_by_key(|(time, _, _)| *time);
    let mut total: u64 = files.iter().map(|(_, size, _)| size).sum();
    for (_, size, file) in files {
        if total <= 64 * 1024 * 1024 { break; }
        if fs::remove_file(file).is_ok() { total -= size; }
    }
}

/// Crops a transparent PNG to its visible pixels and averages it down to fit a centred 64 x 64 icon.
fn emblem_icon(bytes: &[u8]) -> Option<Vec<u8>> {
    let mut decoder = png::Decoder::new(std::io::Cursor::new(bytes));
    decoder.set_transformations(png::Transformations::EXPAND);
    let mut reader = decoder.read_info().ok()?;
    let mut buffer = vec![0; reader.output_buffer_size()];
    let info = reader.next_frame(&mut buffer).ok()?;
    if info.color_type != png::ColorType::Rgba || info.bit_depth != png::BitDepth::Eight { return None; }
    let (w, h) = (info.width as usize, info.height as usize);
    let alpha = |x: usize, y: usize| buffer[(y * w + x) * 4 + 3];
    let (mut x0, mut y0, mut x1, mut y1) = (w, h, 0, 0);
    for y in 0..h { for x in 0..w { if alpha(x, y) > 8 { x0 = x0.min(x); y0 = y0.min(y); x1 = x1.max(x + 1); y1 = y1.max(y + 1); } } }
    if x1 <= x0 || y1 <= y0 { return None; }
    const SIZE: usize = 64;
    let scale = (SIZE as f64 / (x1 - x0) as f64).min(SIZE as f64 / (y1 - y0) as f64);
    let (ow, oh) = (((x1 - x0) as f64 * scale).round().max(1.0) as usize, ((y1 - y0) as f64 * scale).round().max(1.0) as usize);
    let (left, top) = ((SIZE - ow) / 2, (SIZE - oh) / 2);
    let mut out = vec![0u8; SIZE * SIZE * 4];
    for oy in 0..oh {
        for ox in 0..ow {
            // Box average of the source pixels behind this output pixel, weighted by alpha.
            let (sx0, sx1) = (x0 + (ox as f64 / scale) as usize, (x0 + ((ox + 1) as f64 / scale).ceil() as usize).min(x1));
            let (sy0, sy1) = (y0 + (oy as f64 / scale) as usize, (y0 + ((oy + 1) as f64 / scale).ceil() as usize).min(y1));
            let (mut r, mut g, mut b, mut a, mut n) = (0f64, 0f64, 0f64, 0f64, 0f64);
            for sy in sy0..sy1.max(sy0 + 1) { for sx in sx0..sx1.max(sx0 + 1) {
                let i = (sy.min(h - 1) * w + sx.min(w - 1)) * 4;
                let al = f64::from(buffer[i + 3]);
                r += f64::from(buffer[i]) * al; g += f64::from(buffer[i + 1]) * al; b += f64::from(buffer[i + 2]) * al; a += al; n += 1.0;
            } }
            let o = ((top + oy) * SIZE + left + ox) * 4;
            if a > 0.0 { out[o] = (r / a) as u8; out[o + 1] = (g / a) as u8; out[o + 2] = (b / a) as u8; out[o + 3] = (a / n) as u8; }
        }
    }
    let mut encoded = Vec::new();
    let mut encoder = png::Encoder::new(&mut encoded, SIZE as u32, SIZE as u32);
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    encoder.write_header().ok()?.write_image_data(&out).ok()?;
    Some(encoded)
}
