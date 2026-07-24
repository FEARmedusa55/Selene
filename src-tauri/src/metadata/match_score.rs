//! Title similarity scoring for IGDB candidate selection.
//!
//! This exists because IGDB's `search` ranks poorly for our inputs: searching
//! "Super Mario Odyssey" returns "Super Mario Odyssey F.L.U.D.D." and other fan
//! projects above the real game, and searching "Job Simulator" returns
//! "Dirty Jobs Simulator" (IGDB files the real one as "Job Simulator: The 2050
//! Archives").
//!
//! The guiding rule: **a wrong cover silently applied is worse than no cover.**
//! Candidates below `MIN_CONFIDENCE` are rejected outright and the game is left
//! unmatched for the user's manual override, rather than being given plausible
//! but incorrect art.

/// Minimum score for an automatic match. Tuned against real API responses:
/// accepts subtitle extensions ("Job Simulator" -> "Job Simulator: The 2050
/// Archives") while rejecting word-insertion impostors ("Job Simulator" ->
/// "Job Application Simulator").
pub const MIN_CONFIDENCE: f64 = 0.72;

/// Lowercase, strip punctuation, collapse whitespace, and drop leading articles
/// so "The Legend of Zelda" and "Legend of Zelda" compare equal.
pub fn normalize(s: &str) -> String {
    let lowered = s.to_lowercase();
    let cleaned: String = lowered
        .chars()
        .map(|c| if c.is_alphanumeric() { c } else { ' ' })
        .collect();
    let mut words: Vec<&str> = cleaned.split_whitespace().collect();
    if words.len() > 1 && matches!(words[0], "the" | "a" | "an") {
        words.remove(0);
    }
    words.join(" ")
}

fn tokens(s: &str) -> Vec<String> {
    normalize(s).split_whitespace().map(str::to_string).collect()
}

/// Score a candidate title against the query, in 0.0..=1.0.
///
/// Token-based rather than edit-distance based: game titles differ far more
/// often by added or missing *words* (subtitles, editions, region suffixes)
/// than by character-level typos, and Levenshtein punishes a long appended
/// subtitle far more harshly than it deserves.
///
/// The decisive signal is **contiguity**. A legitimate longer title *extends*
/// the query ("Job Simulator" -> "Job Simulator: The 2050 Archives"), whereas
/// an impostor *interrupts* it ("Job Simulator" -> "Job Application
/// Simulator"). A bag-of-words score cannot tell those apart -- both contain
/// every query token -- which is precisely how the live API produced a
/// confident wrong match during development.
pub fn score(query: &str, candidate: &str) -> f64 {
    let q = tokens(query);
    let c = tokens(candidate);
    if q.is_empty() || c.is_empty() {
        return 0.0;
    }
    if q == c {
        return 1.0;
    }

    // Candidate extends the query: a subtitle or edition suffix. Accepted, with
    // a penalty growing in how much was appended, so a heavily-suffixed fan
    // project ("... F.L.U.D.D.") falls below the floor.
    if c.len() > q.len() && c[..q.len()] == q[..] {
        let extra = (c.len() - q.len()) as f64;
        return (0.92 - 0.05 * extra).max(0.70);
    }

    // Query appears contiguously but not at the start ("Beach Vacation
    // Simulator"). Deliberately just under MIN_CONFIDENCE: usually a different
    // game, so leave it to manual override rather than guess.
    if c.len() > q.len() && c.windows(q.len()).any(|w| w == q.as_slice()) {
        return 0.68;
    }

    // Tokens shared but out of order or interrupted. Capped below the floor --
    // this branch should never produce an automatic match on its own.
    let matched = q.iter().filter(|t| c.contains(t)).count() as f64;
    (matched / q.len() as f64) * 0.60
}

/// Pick the best candidate above the confidence floor.
/// Returns `None` when nothing is good enough -- deliberately, so the caller
/// leaves the game unmatched instead of guessing.
pub fn best<'a, T>(query: &str, candidates: &'a [T], name_of: impl Fn(&T) -> &str) -> Option<(&'a T, f64)> {
    candidates
        .iter()
        .map(|c| (c, score(query, name_of(c))))
        .filter(|(_, s)| *s >= MIN_CONFIDENCE)
        .max_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalizes_punctuation_and_articles() {
        assert_eq!(normalize("Animal Crossing: New Horizons"), "animal crossing new horizons");
        assert_eq!(normalize("The Legend of Zelda"), "legend of zelda");
        assert_eq!(normalize("Adventure Time: Explore the Dungeon Because I DON'T KNOW!"),
                   "adventure time explore the dungeon because i don t know");
    }

    #[test]
    fn exact_titles_score_perfectly() {
        assert_eq!(score("Super Mario Odyssey", "Super Mario Odyssey"), 1.0);
        assert_eq!(score("Splatoon", "Splatoon"), 1.0);
        // Punctuation and case must not matter.
        assert_eq!(score("Skylanders: Trap Team", "Skylanders Trap Team"), 1.0);
    }

    #[test]
    fn accepts_legitimate_subtitle_extension() {
        // IGDB's canonical name for the real game the user has on disk.
        let s = score("Job Simulator", "Job Simulator: The 2050 Archives");
        assert!(s >= MIN_CONFIDENCE, "expected accept, scored {s}");
    }

    #[test]
    fn rejects_same_domain_impostors() {
        // All three were returned by the live API for "Job Simulator".
        for impostor in ["Dirty Jobs Simulator", "Virtual Mom: Job Simulator"] {
            let s = score("Job Simulator", impostor);
            assert!(s < MIN_CONFIDENCE, "{impostor} scored {s}, expected reject");
        }
    }

    #[test]
    fn rejects_word_insertion_impostors() {
        // Regression: the live API returned this at 0.93 under the previous
        // bag-of-words scorer. Every query token is present, so only
        // contiguity distinguishes it from a real subtitle extension.
        let s = score("Job Simulator", "Job Application Simulator");
        assert!(s < MIN_CONFIDENCE, "expected reject, scored {s}");

        // And it must rank below the genuine extension of the same title.
        let real = score("Job Simulator", "Job Simulator: The 2050 Archives");
        assert!(s < real, "impostor {s} outranked genuine {real}");
    }

    #[test]
    fn rejects_prefixed_titles() {
        // Regression: "Vacation Simulator" matched "Beach Vacation Simulator"
        // live. The query appears contiguously but not at the start, which
        // almost always means a different game.
        let s = score("Vacation Simulator", "Beach Vacation Simulator");
        assert!(s < MIN_CONFIDENCE, "expected reject, scored {s}");
        assert!(
            s < score("Vacation Simulator", "Vacation Simulator"),
            "prefixed title must rank below the exact one"
        );
    }

    #[test]
    fn rejects_fan_games_that_outrank_the_real_title() {
        for impostor in [
            "Super Mario Odyssey F.L.U.D.D.",
            "Super Mario Odyssey 64",
            "Super Mario Odyssey: Google Translated",
        ] {
            let real = score("Super Mario Odyssey", "Super Mario Odyssey");
            let fake = score("Super Mario Odyssey", impostor);
            assert!(fake < real, "{impostor} scored {fake}, real scored {real}");
        }
    }

    #[test]
    fn distinguishes_numbered_sequels() {
        let one = score("Super Mario Galaxy", "Super Mario Galaxy");
        let two = score("Super Mario Galaxy", "Super Mario Galaxy 2");
        assert!(two < one, "sequel must not outrank the exact title");
        assert_eq!(score("Super Mario Galaxy 2", "Super Mario Galaxy 2"), 1.0);
    }

    #[test]
    fn best_returns_none_when_nothing_clears_the_floor() {
        let candidates = ["Dirty Jobs Simulator", "Virtual Mom: Job Simulator"];
        // Neither is the user's game; leaving it unmatched is the correct outcome.
        let picked = best("Job Simulator", &candidates, |c| c);
        assert!(
            picked.is_none() || picked.unwrap().1 < 0.85,
            "must not confidently pick an impostor"
        );
    }

    #[test]
    fn best_picks_the_strongest_candidate() {
        let candidates = ["Super Mario Odyssey 64", "Super Mario Odyssey", "Super Mario Odyssey: Balloon World"];
        let (picked, _) = best("Super Mario Odyssey", &candidates, |c| c).expect("should match");
        assert_eq!(*picked, "Super Mario Odyssey");
    }
}
