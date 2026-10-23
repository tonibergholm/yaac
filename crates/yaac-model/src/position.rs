use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum PositionError {
    #[error("invalid position key: {0:?}")]
    Invalid(String),
    #[error("no position fits between the given bounds")]
    NoRoom,
}

/// A fractional-index sort key for siblings. Base-62 strings; compared bytewise.
/// Ties between siblings are broken by node id by the caller, not here.
/// Keys never end in `'0'` (the minimum digit), so there is always room below any key.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize)]
#[serde(transparent)]
pub struct Position(String);

/// Digits in ascending byte order, so digit value order equals bytewise order.
const ALPHABET: &[u8; 62] = b"0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz";
const MINIMUM: u8 = 0;
const MIDDLE: u8 = 31;
const MAXIMUM: u8 = 61;
/// Keys longer than this are renumbered before they are used.
const RENUMBER_LEN: usize = 32;

impl Position {
    /// The key for the first element of an empty sibling list.
    pub fn first() -> Self {
        render(&[MIDDLE])
    }

    /// A key that sorts strictly after `before` and strictly before `after` (`None` = open end).
    /// Keys grow by a character when there is no room; callers renumber past 32 characters.
    pub fn between(
        before: Option<&Position>,
        after: Option<&Position>,
    ) -> Result<Self, PositionError> {
        let lower = match before {
            Some(key) => parse_key(key.as_str())?,
            None => Vec::new(),
        };
        let upper = match after {
            Some(key) => Some(parse_key(key.as_str())?),
            None => None,
        };
        let digits = between_digits(&lower, upper.as_deref()).ok_or(PositionError::NoRoom)?;
        Ok(render(&digits))
    }

    /// Validates a key read from storage: non-empty, base-62 only, and not ending in `'0'`.
    pub fn from_raw(raw: String) -> Result<Self, PositionError> {
        match parse(&raw) {
            Some(_) => Ok(Self(raw)),
            None => Err(PositionError::Invalid(raw)),
        }
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// True when the key is too long to use and its siblings should be renumbered.
    pub fn needs_renumber(&self) -> bool {
        self.0.len() > RENUMBER_LEN
    }

    /// `count` short keys in ascending order, evenly spaced over the key space, for renumbering
    /// siblings. Strictly increasing and valid; the first key is never the minimum.
    pub fn evenly_spaced(count: usize) -> Vec<Self> {
        let n = count as u128;
        if n == 0 {
            return Vec::new();
        }
        let mut len: usize = 1;
        let mut space: u128 = ALPHABET.len() as u128;
        while space < n + 1 {
            space *= ALPHABET.len() as u128;
            len += 1;
        }
        // Key i is floor(i * space / (n + 1)), split as q * i + floor(r * i / (n + 1)) so the
        // intermediate products stay inside u128.
        let quotient = space / (n + 1);
        let remainder = space % (n + 1);
        (1..=n)
            .map(|i| {
                let value = i * quotient + (i * remainder) / (n + 1);
                let mut digits = vec![MINIMUM; len];
                let mut rest = value;
                for digit in digits.iter_mut().rev() {
                    *digit = (rest % ALPHABET.len() as u128) as u8;
                    rest /= ALPHABET.len() as u128;
                }
                while digits.last() == Some(&MINIMUM) {
                    digits.pop();
                }
                render(&digits)
            })
            .collect()
    }
}

impl<'de> Deserialize<'de> for Position {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let raw = String::deserialize(deserializer)?;
        Self::from_raw(raw).map_err(serde::de::Error::custom)
    }
}

fn digit_value(byte: u8) -> Option<u8> {
    ALPHABET
        .iter()
        .position(|&digit| digit == byte)
        .and_then(|index| u8::try_from(index).ok())
}

/// Digit values of a valid key, or `None` when the key is empty, has a character outside the
/// alphabet, or ends in the minimum digit.
fn parse(raw: &str) -> Option<Vec<u8>> {
    if raw.is_empty() || raw.ends_with('0') {
        return None;
    }
    raw.bytes().map(digit_value).collect()
}

fn parse_key(raw: &str) -> Result<Vec<u8>, PositionError> {
    parse(raw).ok_or_else(|| PositionError::Invalid(raw.to_owned()))
}

fn render(digits: &[u8]) -> Position {
    Position(
        digits
            .iter()
            .map(|&digit| char::from(ALPHABET[usize::from(digit) % ALPHABET.len()]))
            .collect(),
    )
}

/// Digit at `index`, treating positions past the end as the minimum digit.
fn digit_at(digits: &[u8], index: usize) -> u8 {
    digits.get(index).copied().unwrap_or(MINIMUM)
}

/// The smallest-growing key above `key`, with no upper bound. An empty `key` is the lower bound.
fn above(key: &[u8]) -> Vec<u8> {
    match key.iter().rposition(|&digit| digit != MAXIMUM) {
        Some(index) => {
            let mut out = key[..index].to_vec();
            out.push(key[index] + 1);
            out
        }
        None => {
            let mut out = key.to_vec();
            out.push(MIDDLE);
            out
        }
    }
}

/// A key below `upper`, with zero as the lower bound. `upper` must be non-empty and not end in
/// the minimum digit.
fn below(upper: &[u8]) -> Option<Vec<u8>> {
    let zeros = upper.iter().take_while(|&&digit| digit == MINIMUM).count();
    let mut out = vec![MINIMUM; zeros];
    match upper.get(zeros) {
        Some(&digit) if digit >= 2 => out.push(digit - 1),
        Some(_) => {
            out.push(MINIMUM);
            out.push(MIDDLE);
        }
        None => return None,
    }
    Some(out)
}

/// A digit string strictly between `lower` and `upper` (`None` = no upper bound). An empty
/// `lower` is the lower bound. Returns `None` when `lower >= upper`.
fn between_digits(lower: &[u8], upper: Option<&[u8]>) -> Option<Vec<u8>> {
    let Some(upper) = upper else {
        return Some(above(lower));
    };
    // Keys sharing a prefix keep it; the rest is solved on the first differing digit.
    let common = upper
        .iter()
        .enumerate()
        .take_while(|&(index, &digit)| digit_at(lower, index) == digit)
        .count();
    let mut out = upper[..common].to_vec();
    let upper_rest = &upper[common..];
    let lower_rest = lower.get(common..).unwrap_or(&[]);
    let &upper_first = upper_rest.first()?;
    let Some(&lower_first) = lower_rest.first() else {
        out.extend(below(upper_rest)?);
        return Some(out);
    };
    if lower_first > upper_first {
        return None;
    }
    if upper_first - lower_first > 1 {
        out.push((lower_first + upper_first).div_ceil(2));
    } else if upper_rest.len() > 1 {
        out.push(upper_first);
    } else {
        out.push(lower_first);
        out.extend(above(&lower_rest[1..]));
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    fn key(raw: &str) -> Position {
        Position::from_raw(raw.to_owned()).unwrap()
    }

    /// Keys of 1 to 9 characters, always ending in a non-zero digit.
    fn key_strategy() -> impl Strategy<Value = Position> {
        "[0-9A-Za-z]{0,8}[1-9A-Za-z]".prop_map(|raw| key(&raw))
    }

    #[test]
    fn first_is_the_middle_key() {
        assert_eq!(Position::first().as_str(), "V");
        assert_eq!(Position::between(None, None).unwrap(), Position::first());
    }

    #[test]
    fn between_open_ends_and_neighbours_give_expected_keys() {
        assert_eq!(
            Position::between(Some(&key("az")), None).unwrap().as_str(),
            "b"
        );
        assert_eq!(
            Position::between(None, Some(&key("1"))).unwrap().as_str(),
            "0V"
        );
        assert_eq!(
            Position::between(None, Some(&key("0V"))).unwrap().as_str(),
            "0U"
        );
        assert_eq!(
            Position::between(Some(&key("0V")), None).unwrap().as_str(),
            "0W"
        );
        assert_eq!(
            Position::between(Some(&key("V")), Some(&key("W")))
                .unwrap()
                .as_str(),
            "VV"
        );
        assert_eq!(
            Position::between(Some(&key("V")), Some(&key("Y")))
                .unwrap()
                .as_str(),
            "X"
        );
    }

    #[test]
    fn between_refuses_equal_or_reversed_bounds() {
        assert_eq!(
            Position::between(Some(&key("V")), Some(&key("V"))),
            Err(PositionError::NoRoom)
        );
        assert_eq!(
            Position::between(Some(&key("W")), Some(&key("V"))),
            Err(PositionError::NoRoom)
        );
    }

    #[test]
    fn from_raw_accepts_valid_keys_and_rejects_the_rest() {
        for valid in ["V", "0V", "a", "Z9", "a19"] {
            assert!(Position::from_raw(valid.to_owned()).is_ok(), "{valid}");
        }
        for invalid in ["", "0", "a0", "a-b", " a", "é", "a0b0"] {
            assert_eq!(
                Position::from_raw(invalid.to_owned()),
                Err(PositionError::Invalid(invalid.to_owned())),
                "{invalid}"
            );
        }
    }

    #[test]
    fn needs_renumber_starts_past_32_characters() {
        assert!(!key(&"V".repeat(32)).needs_renumber());
        assert!(key(&"V".repeat(33)).needs_renumber());
    }

    #[test]
    fn evenly_spaced_is_sorted_valid_and_short() {
        assert!(Position::evenly_spaced(0).is_empty());
        for count in [1usize, 2, 3, 30, 61, 62, 63, 500, 3843, 3844, 5000] {
            let keys = Position::evenly_spaced(count);
            assert_eq!(keys.len(), count);
            for pair in keys.windows(2) {
                assert!(
                    pair[0] < pair[1],
                    "{count}: {} !< {}",
                    pair[0].as_str(),
                    pair[1].as_str()
                );
            }
            for key in &keys {
                assert!(Position::from_raw(key.as_str().to_owned()).is_ok());
                assert!(key.as_str().len() <= 3, "{count}: {}", key.as_str());
            }
        }
    }

    #[test]
    fn serde_round_trips_and_rejects_invalid_keys() {
        for raw in ["V", "0V", "a1Z"] {
            let position = key(raw);
            let json = serde_json::to_string(&position).unwrap();
            assert_eq!(json, format!("\"{raw}\""));
            assert_eq!(serde_json::from_str::<Position>(&json).unwrap(), position);
        }
        for bad in ["\"\"", "\"a0\"", "\"a-\"", "5"] {
            assert!(serde_json::from_str::<Position>(bad).is_err(), "{bad}");
        }
    }

    proptest! {
        #[test]
        fn between_is_strictly_inside_its_bounds(a in key_strategy(), b in key_strategy()) {
            prop_assume!(a != b);
            let (lo, hi) = if a < b { (a, b) } else { (b, a) };
            let mid = Position::between(Some(&lo), Some(&hi)).unwrap();
            prop_assert!(lo < mid && mid < hi);
            prop_assert!(Position::from_raw(mid.as_str().to_owned()).is_ok());
            prop_assert!(mid.as_str().len() <= lo.as_str().len().max(hi.as_str().len()) + 1);

            let above = Position::between(Some(&lo), None).unwrap();
            prop_assert!(above > lo);
            let below = Position::between(None, Some(&hi)).unwrap();
            prop_assert!(below < hi);
        }

        #[test]
        fn repeated_insertion_before_one_key_never_fails(
            lower in key_strategy(),
            upper in key_strategy(),
            steps in 1usize..300,
        ) {
            prop_assume!(lower < upper);
            let mut previous = lower;
            for _ in 0..steps {
                let next = Position::between(Some(&previous), Some(&upper)).unwrap();
                prop_assert!(previous < next && next < upper);
                prop_assert!(
                    next.as_str().len() <= previous.as_str().len().max(upper.as_str().len()) + 1
                );
                previous = next;
            }
        }
    }
}
