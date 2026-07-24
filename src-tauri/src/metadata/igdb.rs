//! IGDB metadata client.
//!
//! Auth is Twitch OAuth client-credentials: exchange Client ID + Secret for a
//! bearer token that lasts ~60 days, then send `Client-ID` and
//! `Authorization: Bearer` on every request. The token is cached in the
//! `settings` table so a restart does not burn a new one.
//!
//! Query strategy is tiered, because IGDB's own `search` ranks badly for our
//! inputs (see `match_score`):
//!   1. Exact `where name = "..."` (+ platform filter) -- highest confidence.
//!   2. Contains `where name ~ *"..."*` (+ platform) -- catches subtitle forms
//!      like "Job Simulator: The 2050 Archives".
//!   3. `search` with client-side scoring -- last resort.
//! Anything failing the confidence floor is reported as unmatched rather than
//! given wrong art.
//!
//! NOTE: `search` and `where` cannot be combined in one Apicalypse query --
//! IGDB silently returns zero rows. Tiers 1-2 use `where`; tier 3 uses `search`
//! alone and filters in-process.

use crate::model::Platform;
use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use super::match_score;

const TWITCH_TOKEN_URL: &str = "https://id.twitch.tv/oauth2/token";
const IGDB_GAMES_URL: &str = "https://api.igdb.com/v4/games";

/// IGDB allows 4 requests/second. Spacing requests by 260ms keeps us safely
/// under it without needing a token bucket.
///
/// NOTE: this limiter is per-client. IGDB's quota is per-credential, so several
/// `IgdbClient` instances sharing one Client ID can collectively exceed it --
/// running the live tests in parallel does exactly that. The app holds a single
/// client, so this is adequate today; if concurrent scans are ever added, the
/// limiter must move to shared state.
const REQUEST_SPACING: Duration = Duration::from_millis(260);

/// Refresh this long before actual expiry so a long scan cannot have the token
/// die underneath it.
const REFRESH_MARGIN_SECS: u64 = 24 * 60 * 60;

/// IGDB platform IDs. Verified against the live API.
fn igdb_platform_id(p: Platform) -> Option<u32> {
    Some(match p {
        Platform::Switch => 130,
        Platform::WiiU => 41,
        Platform::Wii => 5,
        Platform::GameCube => 21,
        Platform::Pc => 6,
    })
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Credentials {
    pub client_id: String,
    pub client_secret: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CachedToken {
    pub access_token: String,
    /// Unix seconds.
    pub expires_at: u64,
}

impl CachedToken {
    fn is_usable(&self) -> bool {
        now_secs() + REFRESH_MARGIN_SECS < self.expires_at
    }
}

fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

#[derive(Debug, Deserialize)]
struct TwitchTokenResponse {
    access_token: String,
    expires_in: u64,
}

#[derive(Debug, Clone, Deserialize)]
struct ImageRef {
    image_id: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
struct IgdbGame {
    name: String,
    cover: Option<ImageRef>,
    #[serde(default)]
    artworks: Vec<ImageRef>,
    #[serde(default)]
    screenshots: Vec<ImageRef>,
    /// Used to break ties between identically-named entries. IGDB holds three
    /// separate games called exactly "Paper Mario: The Thousand-Year Door"
    /// (GameCube original, Switch remake, and a third), all scoring 1.00 -- so
    /// name similarity alone would pick the remake's art for a GameCube disc.
    #[serde(default)]
    platforms: Vec<u32>,
}

/// What the matcher resolved for one game.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Artwork {
    pub matched_name: String,
    pub confidence: f64,
    pub cover_url: Option<String>,
    pub hero_url: Option<String>,
}

/// Build a full-size image URL. IGDB's ids come back sized for thumbnails, so
/// the size token is swapped for a large one.
/// `t_cover_big` = 264x374, `t_1080p` = 1920x1080.
pub fn image_url(image_id: &str, size: &str) -> String {
    format!("https://images.igdb.com/igdb/image/upload/{size}/{image_id}.jpg")
}

pub struct IgdbClient {
    http: reqwest::Client,
    creds: Credentials,
    token: Option<CachedToken>,
    last_request: Option<std::time::Instant>,
}

impl IgdbClient {
    pub fn new(creds: Credentials, cached: Option<CachedToken>) -> Self {
        Self {
            http: reqwest::Client::builder()
                .timeout(Duration::from_secs(20))
                .build()
                .expect("building HTTP client"),
            creds,
            token: cached,
            last_request: None,
        }
    }

    /// Current token, refreshing if absent or near expiry. Callers should
    /// persist the returned token via [`CachedToken`] so restarts reuse it.
    pub async fn token(&mut self) -> Result<CachedToken> {
        if let Some(t) = &self.token {
            if t.is_usable() {
                return Ok(t.clone());
            }
        }
        let resp = self
            .http
            .post(TWITCH_TOKEN_URL)
            .query(&[
                ("client_id", self.creds.client_id.as_str()),
                ("client_secret", self.creds.client_secret.as_str()),
                ("grant_type", "client_credentials"),
            ])
            .send()
            .await
            .context("requesting Twitch OAuth token")?;

        if !resp.status().is_success() {
            let status = resp.status();
            // Deliberately does not echo the response body: it can contain the
            // submitted credentials.
            bail!("Twitch OAuth rejected the credentials (HTTP {status})");
        }

        let body: TwitchTokenResponse = resp.json().await.context("parsing Twitch token")?;
        let token = CachedToken {
            access_token: body.access_token,
            expires_at: now_secs() + body.expires_in,
        };
        self.token = Some(token.clone());
        Ok(token)
    }

    async fn respect_rate_limit(&mut self) {
        if let Some(last) = self.last_request {
            let elapsed = last.elapsed();
            if elapsed < REQUEST_SPACING {
                tokio::time::sleep(REQUEST_SPACING - elapsed).await;
            }
        }
        self.last_request = Some(std::time::Instant::now());
    }

    async fn query(&mut self, body: String) -> Result<Vec<IgdbGame>> {
        let token = self.token().await?;
        self.respect_rate_limit().await;

        let resp = self
            .http
            .post(IGDB_GAMES_URL)
            .header("Client-ID", &self.creds.client_id)
            .header("Authorization", format!("Bearer {}", token.access_token))
            .body(body)
            .send()
            .await
            .context("querying IGDB")?;

        if !resp.status().is_success() {
            let status = resp.status();
            let detail = resp.text().await.unwrap_or_default();
            bail!("IGDB query failed (HTTP {status}): {detail}");
        }
        resp.json().await.context("parsing IGDB response")
    }

    /// Resolve artwork for a title. `Ok(None)` means "no confident match" --
    /// a normal outcome, not an error, and the cue for manual override.
    pub async fn find_artwork(
        &mut self,
        title: &str,
        platform: Platform,
    ) -> Result<Option<Artwork>> {
        let esc = title.replace('"', "\\\"");
        let fields =
            "fields name,platforms,cover.image_id,artworks.image_id,screenshots.image_id;";
        let plat = igdb_platform_id(platform)
            .map(|id| format!(" & platforms = ({id})"))
            .unwrap_or_default();

        // Platform is a preference, not a requirement. It disambiguates
        // same-named titles, but IGDB's platform lists are incomplete in ways
        // that matter here: VR games such as Vacation Simulator do not list
        // platform 6 (Windows) at all, so filtering on it discarded the correct
        // match and left a near-miss ("Beach Vacation Simulator") to win.
        // Each tier is therefore retried unfiltered before widening further.
        let tiers = [
            format!("where name = \"{esc}\"{plat}; {fields} limit 5;"),
            format!("where name = \"{esc}\"; {fields} limit 5;"),
            format!("where name ~ *\"{esc}\"*{plat}; {fields} limit 25;"),
            format!("where name ~ *\"{esc}\"*; {fields} limit 25;"),
            // `search` cannot be combined with `where`; filtered in-process.
            format!("search \"{esc}\"; {fields} limit 25;"),
        ];

        let want_platform = igdb_platform_id(platform);

        // Rank by score, then prefer an entry that actually has cover art, then
        // one on the right platform.
        //
        // Cover art is a tie-break because IGDB carries artless stub entries for
        // editions and bundles: "Skylanders SuperChargers: Dark Edition Starter
        // Pack" outranked nothing, but it was picked over the real game and
        // produced a title with no art at all.
        let pick = |cands: &[IgdbGame]| -> Option<(usize, f64)> {
            cands
                .iter()
                .enumerate()
                .map(|(i, g)| (i, match_score::score(title, &g.name)))
                .filter(|(_, s)| *s >= match_score::MIN_CONFIDENCE)
                .max_by(|a, b| {
                    let art = |i: usize| cands[i].cover.as_ref().and_then(|c| c.image_id.as_ref()).is_some();
                    let plat = |i: usize| {
                        want_platform.map(|p| cands[i].platforms.contains(&p)).unwrap_or(false)
                    };
                    a.1.partial_cmp(&b.1)
                        .unwrap_or(std::cmp::Ordering::Equal)
                        .then(art(a.0).cmp(&art(b.0)))
                        .then(plat(a.0).cmp(&plat(b.0)))
                })
        };

        // Accumulate across tiers rather than stopping at the first that yields
        // *any* acceptable match.
        //
        // The narrow `where name ~ *"..."*` tier is literal and punctuation
        // sensitive: querying "Skylanders SuperChargers" never matches IGDB's
        // "Skylanders: SuperChargers", but does match a Dark Edition bundle. So
        // a mediocre early hit was ending the search before the `search` tier
        // could find the real game. Only a near-exact match stops the walk now.
        const STOP_EARLY_AT: f64 = 0.95;

        let mut candidates: Vec<IgdbGame> = Vec::new();
        for tier in tiers {
            let mut found = self.query(tier).await?;
            candidates.append(&mut found);
            if pick(&candidates).is_some_and(|(_, s)| s >= STOP_EARLY_AT) {
                break;
            }
        }

        Ok(pick(&candidates).map(|(idx, confidence)| {
            let game = &candidates[idx];
            {
                // Prefer artwork for the hero; fall back to a screenshot. IGDB
                // has no curated hero-banner asset, which is why a manual
                // override always stays available.
                let hero = game
                    .artworks
                    .iter()
                    .chain(game.screenshots.iter())
                    .find_map(|a| a.image_id.as_deref())
                    .map(|id| image_url(id, "t_1080p"));

                Artwork {
                    matched_name: game.name.clone(),
                    confidence,
                    cover_url: game
                        .cover
                        .as_ref()
                        .and_then(|c| c.image_id.as_deref())
                        .map(|id| image_url(id, "t_cover_big")),
                    hero_url: hero,
                }
            }
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builds_large_image_urls() {
        assert_eq!(
            image_url("co1mxf", "t_cover_big"),
            "https://images.igdb.com/igdb/image/upload/t_cover_big/co1mxf.jpg"
        );
        assert_eq!(
            image_url("ar8lg", "t_1080p"),
            "https://images.igdb.com/igdb/image/upload/t_1080p/ar8lg.jpg"
        );
    }

    #[test]
    fn maps_every_platform_to_an_igdb_id() {
        // Verified against the live API during development.
        assert_eq!(igdb_platform_id(Platform::Switch), Some(130));
        assert_eq!(igdb_platform_id(Platform::WiiU), Some(41));
        assert_eq!(igdb_platform_id(Platform::Wii), Some(5));
        assert_eq!(igdb_platform_id(Platform::GameCube), Some(21));
        assert_eq!(igdb_platform_id(Platform::Pc), Some(6));
    }

    #[test]
    fn token_expiry_accounts_for_refresh_margin() {
        let fresh = CachedToken {
            access_token: "x".into(),
            expires_at: now_secs() + 60 * 24 * 60 * 60,
        };
        assert!(fresh.is_usable());

        // Inside the refresh margin: must be treated as unusable.
        let nearly_expired = CachedToken {
            access_token: "x".into(),
            expires_at: now_secs() + REFRESH_MARGIN_SECS / 2,
        };
        assert!(!nearly_expired.is_usable());

        let expired = CachedToken { access_token: "x".into(), expires_at: 0 };
        assert!(!expired.is_usable());
    }
}
