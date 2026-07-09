use rand::random;

/// # Panics
/// Panics when String index is out of range.
#[must_use]
pub fn generate_name(len: u8) -> String {
    let vowels = [b'a', b'e', b'i', b'o', b'u'];
    let consonants: Vec<u8> = (b'a'..=b'z').filter(|c| !vowels.contains(c)).collect();
    let mut name = vec![];
    let mut next_is_vowel = random::<bool>();
    for _ in 0..len {
        let char = if next_is_vowel {
            vowels[rand::random_range(0..vowels.len())]
        } else {
            consonants[rand::random_range(0..consonants.len())]
        };
        name.push(char);
        next_is_vowel = !next_is_vowel;
    }
    String::from_utf8(name).unwrap()
}
