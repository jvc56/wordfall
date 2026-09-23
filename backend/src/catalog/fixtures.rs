//! The fixture catalog (PLAN.md § The fixture catalog), parsed directly from
//! `fixtures/catalog/` by the backend unit tests.

use std::path::PathBuf;
use std::sync::Arc;

use super::index::{LeaveSetIndex, LexiconIndex, RawWord};
use super::tiles::Distribution;
use super::upload;

pub struct Fixtures {
    pub english: Arc<Distribution>,
    pub catalan: Arc<Distribution>,
    pub en: Arc<LexiconIndex>,
    pub en_old: Arc<LexiconIndex>,
    pub ca: Arc<LexiconIndex>,
    pub en_leaves: Arc<LeaveSetIndex>,
    pub ca_leaves: Arc<LeaveSetIndex>,
}

fn path(file: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../fixtures/catalog")
        .join(file)
}

fn distribution(id: i16, name: &str) -> Arc<Distribution> {
    let bytes = std::fs::read(path(&format!("{name}.csv"))).unwrap();
    let parsed = upload::parse_distribution(&bytes).unwrap();
    Arc::new(Distribution::new(id, name, parsed.tiles))
}

fn lexicon(id: i16, name: &str, dist: &Arc<Distribution>) -> Arc<LexiconIndex> {
    let bytes = std::fs::read(path(&format!("{name}.tsv"))).unwrap();
    let words = upload::parse_lexicon(&bytes, dist).unwrap();
    let raw = words
        .into_iter()
        .map(|w| RawWord {
            tiles: w.tiles,
            playability: w.playability,
            definition: w.definition,
        })
        .collect();
    Arc::new(LexiconIndex::build(id, name, dist.clone(), raw))
}

fn leaves(id: i32, file: &str, lexicon: &LexiconIndex) -> Arc<LeaveSetIndex> {
    let bytes = std::fs::read(path(file)).unwrap();
    let parsed = upload::parse_leaves(&bytes, &lexicon.distribution).unwrap();
    Arc::new(LeaveSetIndex::build(
        id,
        lexicon,
        parsed.into_iter().map(|l| (l.tiles, l.value)).collect(),
    ))
}

pub fn load() -> Fixtures {
    let english = distribution(1, "english");
    let catalan = distribution(2, "catalan");
    let en = lexicon(1, "EN-FIX", &english);
    let en_old = lexicon(2, "EN-FIX-OLD", &english);
    let ca = lexicon(3, "CA-FIX", &catalan);
    let en_leaves = leaves(1, "EN-FIX-leaves.csv", &en);
    let ca_leaves = leaves(2, "CA-FIX-leaves.csv", &ca);
    Fixtures {
        english,
        catalan,
        en,
        en_old,
        ca,
        en_leaves,
        ca_leaves,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::catalog::pos::PartOfSpeech;
    use crate::catalog::tiles::Tile;

    fn t(d: &Distribution, s: &str) -> Vec<Tile> {
        d.parse_magpie(s, true).unwrap()
    }

    #[test]
    fn word_attributes() {
        let f = load();
        let d = &f.english;
        let exit = f.en.find(&t(d, "EXIT")).unwrap();
        assert_eq!(
            (
                exit.length,
                exit.num_vowels,
                exit.num_unique_letters,
                exit.point_value
            ),
            (4, 2, 4, 11)
        );
        assert_eq!(exit.num_anagrams, 1);
        assert_eq!(
            exit.pos,
            PartOfSpeech::Noun.bit() | PartOfSpeech::Verb.bit()
        );
        assert!(exit.front_hooks.is_empty() && exit.back_hooks.is_empty());
        assert_eq!(
            exit.counts.len(),
            d.len(),
            "a count vector sized to the distribution"
        );

        // Inner hooks: SPORT (PORT), SPORTS (SPORT); a one-tile word has none.
        let sport = f.en.find(&t(d, "SPORT")).unwrap();
        assert!(sport.has_front_inner_hook && !sport.has_back_inner_hook);
        assert_eq!(&*sport.back_hooks, &t(d, "S")[..]);
        let sports = f.en.find(&t(d, "SPORTS")).unwrap();
        assert!(sports.has_back_inner_hook && !sports.has_front_inner_hook);
        let port = f.en.find(&t(d, "PORT")).unwrap();
        assert_eq!(&*port.front_hooks, &t(d, "S")[..]);
        let a = f.en.find(&t(d, "A")).unwrap();
        assert!(!a.has_front_inner_hook && !a.has_back_inner_hook);
        // Hooks in tile order: AA AB AL AN AT AX, and AA BA LA TA ZA.
        assert_eq!(&*a.back_hooks, &t(d, "ABLNTX")[..]);
        assert_eq!(&*a.front_hooks, &t(d, "ABLTZ")[..]);

        // AEINRST: nine anagrams, the fixture's most.
        let retains = f.en.find(&t(d, "RETAINS")).unwrap();
        assert_eq!(retains.num_anagrams, 9);
        assert_eq!(f.en.max_num_anagrams, 9);
        let answers: Vec<String> =
            f.en.anagrams(&t(d, "AEINRST"))
                .map(|w| d.to_magpie(&w.tiles))
                .collect();
        assert_eq!(
            answers,
            [
                "ANESTRI", "ANTSIER", "NASTIER", "RATINES", "RETAINS", "RETINAS", "RETSINA",
                "STAINER", "STEARIN"
            ]
        );
        assert_eq!(f.en.find(&t(d, "AT")).unwrap().num_anagrams, 2);
    }

    #[test]
    fn combinations_by_hand() {
        let f = load();
        let d = &f.english;
        // QI: 9 with no blank; 18 + 2 with one; 1 with two.
        assert_eq!(f.en.find(&t(d, "QI")).unwrap().combinations, 30);
        // AA: C(9,2) + 2·9 + 1.
        assert_eq!(f.en.find(&t(d, "AA")).unwrap().combinations, 55);
    }

    #[test]
    fn ranks_are_consistent_on_ties() {
        let f = load();
        for lex in [&f.en, &f.en_old, &f.ca] {
            for bucket in &lex.by_length {
                let n = bucket.len() as u32;
                let mut prob: Vec<_> = bucket.iter().map(|&i| &lex.words[i as usize]).collect();
                prob.sort_by_key(|w| w.probability_order);
                assert_eq!(
                    prob.iter().map(|w| w.probability_order).collect::<Vec<_>>(),
                    (1..=n).collect::<Vec<_>>()
                );
                for pair in prob.windows(2) {
                    let (a, b) = (pair[0], pair[1]);
                    assert!(a.combinations >= b.combinations);
                    if a.combinations == b.combinations {
                        let key = |w: &crate::catalog::index::WordEntry| {
                            (
                                lex.alphagrams[w.alphagram as usize].clone(),
                                w.tiles.clone(),
                            )
                        };
                        assert!(key(a) < key(b), "ties broken by alphagram, then word");
                    }
                }
                for w in &prob {
                    let same: Vec<u32> = prob
                        .iter()
                        .filter(|x| x.combinations == w.combinations)
                        .map(|x| x.probability_order)
                        .collect();
                    assert_eq!(w.min_probability_order, *same.iter().min().unwrap());
                    assert_eq!(w.max_probability_order, *same.iter().max().unwrap());
                }
                let mut play: Vec<_> = bucket.iter().map(|&i| &lex.words[i as usize]).collect();
                play.sort_by_key(|w| w.playability_order);
                for pair in play.windows(2) {
                    assert!(pair[0].playability >= pair[1].playability);
                }
                for w in &play {
                    let same: Vec<u32> = play
                        .iter()
                        .filter(|x| x.playability == w.playability)
                        .map(|x| x.playability_order)
                        .collect();
                    assert_eq!(w.min_playability_order, *same.iter().min().unwrap());
                    assert_eq!(w.max_playability_order, *same.iter().max().unwrap());
                }
            }
        }
        // The nine AEINRST anagrams tie on combinations and share one range.
        let f2 = load();
        let d = &f2.english;
        let a = f2.en.find(&t(d, "ANESTRI")).unwrap();
        let s = f2.en.find(&t(d, "STEARIN")).unwrap();
        assert_eq!(a.max_probability_order - a.min_probability_order, 8);
        assert_eq!(
            (a.min_probability_order, a.max_probability_order),
            (s.min_probability_order, s.max_probability_order)
        );
        assert!(a.probability_order < s.probability_order);
    }

    #[test]
    fn leave_attributes() {
        let f = load();
        let d = &f.english;
        let l = f.en_leaves.find(&t(d, "?EIRS")).unwrap();
        assert_eq!(
            (l.length, l.num_vowels, l.num_unique_letters, l.point_value),
            (5, 2, 5, 4)
        );
        // The blank is an ordinary tile at the bag's count: 2·12·9·6·4.
        assert_eq!(l.combinations, 5184);
        assert_eq!(l.num_anagrams, 0, "a leave holding a blank");
        assert_eq!(l.value, 34.1);
        assert_eq!(f.en_leaves.find(&t(d, "EITX")).unwrap().num_anagrams, 1);
        assert_eq!(f.en_leaves.find(&t(d, "AT")).unwrap().num_anagrams, 2);
        assert_eq!(f.en_leaves.max_num_anagrams, 2);
        // Catalan leaves in canonical order.
        let c = &f.catalan;
        let cl = f
            .ca_leaves
            .find(&c.parse_magpie("?A[L·L]S", true).unwrap())
            .unwrap();
        assert_eq!(cl.length, 4);
        assert_eq!(c.to_magpie(&cl.tiles), "?A[L·L]S");
    }

    #[test]
    fn catalan_words_count_tiles_not_characters() {
        let f = load();
        let c = &f.catalan;
        let anys =
            f.ca.find(&c.parse_magpie("A[NY]S", false).unwrap())
                .unwrap();
        assert_eq!(anys.length, 3);
        assert_eq!(f.ca.by_length[15].len(), 4, "the four fifteen-tile words");
        assert!(
            !f.ca
                .find(&c.parse_magpie("[QU]E[L·L]A", false).unwrap())
                .unwrap()
                .has_front_inner_hook
        );
    }
}
