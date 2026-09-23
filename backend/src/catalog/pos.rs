//! Parts of speech from definition tags (PLAN.md § Filter reference, Part of
//! Speech): Zyzzyva matches `[tag ` (tag, then a space) or `[tag]`, so
//! `[n -S]` and `[n]` are nouns and `[interj]` is not a noun.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, sqlx::Type)]
#[serde(rename_all = "snake_case")]
#[sqlx(type_name = "part_of_speech", rename_all = "snake_case")]
pub enum PartOfSpeech {
    Adjective,
    Adverb,
    Conjunction,
    DefiniteArticle,
    IndefiniteArticle,
    Interjection,
    Noun,
    Preposition,
    Pronoun,
    Verb,
}

impl PartOfSpeech {
    pub const ALL: [PartOfSpeech; 10] = [
        PartOfSpeech::Adjective,
        PartOfSpeech::Adverb,
        PartOfSpeech::Conjunction,
        PartOfSpeech::DefiniteArticle,
        PartOfSpeech::IndefiniteArticle,
        PartOfSpeech::Interjection,
        PartOfSpeech::Noun,
        PartOfSpeech::Preposition,
        PartOfSpeech::Pronoun,
        PartOfSpeech::Verb,
    ];

    pub fn tag(self) -> &'static str {
        match self {
            PartOfSpeech::Adjective => "adj",
            PartOfSpeech::Adverb => "adv",
            PartOfSpeech::Conjunction => "conj",
            PartOfSpeech::DefiniteArticle => "definite_article",
            PartOfSpeech::IndefiniteArticle => "indefinite_article",
            PartOfSpeech::Interjection => "interj",
            PartOfSpeech::Noun => "n",
            PartOfSpeech::Preposition => "prep",
            PartOfSpeech::Pronoun => "pron",
            PartOfSpeech::Verb => "v",
        }
    }

    pub fn bit(self) -> u16 {
        1 << (self as u16)
    }
}

/// The parts of speech a definition's tags name, as a bitmask.
pub fn parse(definition: &str) -> u16 {
    let mut mask = 0;
    for pos in PartOfSpeech::ALL {
        let tag = pos.tag();
        let spaced = format!("[{tag} ");
        let closed = format!("[{tag}]");
        if definition.contains(&spaced) || definition.contains(&closed) {
            mask |= pos.bit();
        }
    }
    mask
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tags_need_a_space_or_a_bracket_after_them() {
        let m =
            parse("an interjection expressing surprise [interj] / to exclaim [v -ED, -ING, -S]");
        assert_eq!(
            m,
            PartOfSpeech::Interjection.bit() | PartOfSpeech::Verb.bit()
        );
        assert_eq!(parse("a rock [n -S]"), PartOfSpeech::Noun.bit());
        assert_eq!(parse("a rock [n]"), PartOfSpeech::Noun.bit());
        assert_eq!(parse("[nonsense]"), 0);
        assert_eq!(
            parse("the [definite_article]"),
            PartOfSpeech::DefiniteArticle.bit()
        );
    }
}
