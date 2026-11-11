//! Public wallet addresses: network detection, checksums and the secret guard.
//! Loupist only reads public balances; anything that looks like a recovery
//! phrase or private key is refused before it is stored, logged or echoed.
use sha2::{Digest, Sha256};

#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "snake_case")]
pub enum WalletFamily {
    Bitcoin,
    /// One address on Ethereum, Base, Arbitrum, Optimism and Polygon.
    Evm,
    Solana,
}

impl WalletFamily {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Bitcoin => "bitcoin",
            Self::Evm => "evm",
            Self::Solana => "solana",
        }
    }
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "bitcoin" => Some(Self::Bitcoin),
            "evm" => Some(Self::Evm),
            "solana" => Some(Self::Solana),
            _ => None,
        }
    }
}

#[derive(Debug, PartialEq, Eq)]
pub enum AddressCheck {
    /// The canonical form to store: lowercase bech32, EIP-55 for EVM.
    Valid(WalletFamily, String),
    /// A recovery phrase or private key. The caller must drop the text.
    Secret,
    Unsupported,
}

const BASE58: &[u8; 58] = b"123456789ABCDEFGHJKLMNPQRSTUVWXYZabcdefghijkmnopqrstuvwxyz";

pub(super) fn base58_decode(text: &str) -> Option<Vec<u8>> {
    if text.is_empty() || text.len() > 128 {
        return None;
    }
    let mut bytes: Vec<u8> = Vec::with_capacity(text.len());
    for character in text.bytes() {
        let mut carry = BASE58.iter().position(|item| *item == character)? as u32;
        for byte in bytes.iter_mut().rev() {
            carry += u32::from(*byte) * 58;
            *byte = (carry & 0xff) as u8;
            carry >>= 8;
        }
        while carry > 0 {
            bytes.insert(0, (carry & 0xff) as u8);
            carry >>= 8;
        }
    }
    let zeros = text.bytes().take_while(|byte| *byte == b'1').count();
    let mut decoded = vec![0; zeros];
    decoded.extend(bytes.into_iter().skip_while(|byte| *byte == 0));
    Some(decoded)
}

fn base58check(text: &str) -> Option<Vec<u8>> {
    let decoded = base58_decode(text)?;
    if decoded.len() < 5 {
        return None;
    }
    let (payload, checksum) = decoded.split_at(decoded.len() - 4);
    let digest = Sha256::digest(Sha256::digest(payload));
    (digest[..4] == *checksum).then(|| payload.to_vec())
}

const BECH32_CHARSET: &[u8; 32] = b"qpzry9x8gf2tvdw0s3jn54khce6mua7l";

fn bech32_polymod(values: &[u8]) -> u32 {
    const GENERATOR: [u32; 5] = [0x3b6a57b2, 0x26508e6d, 0x1ea119fa, 0x3d4233dd, 0x2a1462b3];
    let mut checksum = 1u32;
    for value in values {
        let top = checksum >> 25;
        checksum = ((checksum & 0x1ffffff) << 5) ^ u32::from(*value);
        for (index, generator) in GENERATOR.iter().enumerate() {
            if (top >> index) & 1 == 1 {
                checksum ^= generator;
            }
        }
    }
    checksum
}

/// BIP-173 / BIP-350 segwit address on Bitcoin mainnet (`bc`).
fn segwit_mainnet(text: &str) -> Option<String> {
    if text.len() > 90
        || (text.bytes().any(|b| b.is_ascii_lowercase())
            && text.bytes().any(|b| b.is_ascii_uppercase()))
    {
        return None;
    }
    let text = text.to_ascii_lowercase();
    let separator = text.rfind('1')?;
    let (hrp, data) = (&text[..separator], &text[separator + 1..]);
    if hrp != "bc" || data.len() < 6 {
        return None;
    }
    let values = data
        .bytes()
        .map(|byte| {
            BECH32_CHARSET
                .iter()
                .position(|item| *item == byte)
                .map(|index| index as u8)
        })
        .collect::<Option<Vec<_>>>()?;
    let mut expanded: Vec<u8> = hrp.bytes().map(|byte| byte >> 5).collect();
    expanded.push(0);
    expanded.extend(hrp.bytes().map(|byte| byte & 31));
    expanded.extend(&values);
    let constant = bech32_polymod(&expanded);
    let words = &values[..values.len() - 6];
    let (&version, program_words) = words.split_first()?;
    let expected = if version == 0 { 1 } else { 0x2bc8_30a3 };
    if constant != expected || version > 16 {
        return None;
    }
    // Regroup 5-bit words into bytes without padding.
    let (mut accumulator, mut bits, mut program) = (0u32, 0u32, Vec::new());
    for word in program_words {
        accumulator = ((accumulator << 5) | u32::from(*word)) & 0xffff;
        bits += 5;
        while bits >= 8 {
            bits -= 8;
            program.push(((accumulator >> bits) & 0xff) as u8);
        }
    }
    if bits >= 5 || (accumulator & ((1 << bits) - 1)) != 0 {
        return None;
    }
    let valid_length = (2..=40).contains(&program.len())
        && (version != 0 || program.len() == 20 || program.len() == 32);
    valid_length.then_some(text)
}

const KECCAK_ROUNDS: [u64; 24] = [
    0x0000000000000001,
    0x0000000000008082,
    0x800000000000808a,
    0x8000000080008000,
    0x000000000000808b,
    0x0000000080000001,
    0x8000000080008081,
    0x8000000000008009,
    0x000000000000008a,
    0x0000000000000088,
    0x0000000080008009,
    0x000000008000000a,
    0x000000008000808b,
    0x800000000000008b,
    0x8000000000008089,
    0x8000000000008003,
    0x8000000000008002,
    0x8000000000000080,
    0x000000000000800a,
    0x800000008000000a,
    0x8000000080008081,
    0x8000000000008080,
    0x0000000080000001,
    0x8000000080008008,
];
const KECCAK_ROTATIONS: [u32; 24] = [
    1, 3, 6, 10, 15, 21, 28, 36, 45, 55, 2, 14, 27, 41, 56, 8, 25, 43, 62, 18, 39, 61, 20, 44,
];
const KECCAK_LANES: [usize; 24] = [
    10, 7, 11, 17, 18, 3, 5, 16, 8, 21, 24, 4, 15, 23, 19, 13, 12, 2, 20, 14, 22, 9, 6, 1,
];

fn keccak_f(state: &mut [u64; 25]) {
    for round in KECCAK_ROUNDS {
        let mut columns = [0u64; 5];
        for (index, column) in columns.iter_mut().enumerate() {
            *column = state[index]
                ^ state[index + 5]
                ^ state[index + 10]
                ^ state[index + 15]
                ^ state[index + 20];
        }
        for index in 0..5 {
            let mix = columns[(index + 4) % 5] ^ columns[(index + 1) % 5].rotate_left(1);
            for row in (0..25).step_by(5) {
                state[row + index] ^= mix;
            }
        }
        let mut carried = state[1];
        for (lane, rotation) in KECCAK_LANES.iter().zip(KECCAK_ROTATIONS) {
            let next = state[*lane];
            state[*lane] = carried.rotate_left(rotation);
            carried = next;
        }
        for row in (0..25).step_by(5) {
            let lanes = [
                state[row],
                state[row + 1],
                state[row + 2],
                state[row + 3],
                state[row + 4],
            ];
            for index in 0..5 {
                state[row + index] =
                    lanes[index] ^ (!lanes[(index + 1) % 5] & lanes[(index + 2) % 5]);
            }
        }
        state[0] ^= round;
    }
}

/// Original Keccak-256 (Ethereum), not NIST SHA3-256.
pub(super) fn keccak256(data: &[u8]) -> [u8; 32] {
    const RATE: usize = 136;
    let mut state = [0u64; 25];
    let absorb = |state: &mut [u64; 25], block: &[u8]| {
        for (lane, chunk) in state.iter_mut().zip(block.chunks_exact(8)) {
            *lane ^= u64::from_le_bytes(chunk.try_into().expect("8-byte lane"));
        }
        keccak_f(state);
    };
    let mut blocks = data.chunks_exact(RATE);
    for block in &mut blocks {
        absorb(&mut state, block);
    }
    let remainder = blocks.remainder();
    let mut last = [0u8; RATE];
    last[..remainder.len()].copy_from_slice(remainder);
    last[remainder.len()] ^= 0x01;
    last[RATE - 1] ^= 0x80;
    absorb(&mut state, &last);
    let mut output = [0u8; 32];
    for (chunk, lane) in output.chunks_exact_mut(8).zip(state) {
        chunk.copy_from_slice(&lane.to_le_bytes());
    }
    output
}

fn eip55(lower_hex: &str) -> String {
    let hash = keccak256(lower_hex.as_bytes());
    let mut checksummed = String::from("0x");
    for (index, character) in lower_hex.chars().enumerate() {
        let nibble = (hash[index / 2] >> (if index % 2 == 0 { 4 } else { 0 })) & 0x0f;
        checksummed.push(if character.is_ascii_alphabetic() && nibble >= 8 {
            character.to_ascii_uppercase()
        } else {
            character
        });
    }
    checksummed
}

fn evm(text: &str) -> Option<String> {
    let hex = text
        .strip_prefix("0x")
        .or_else(|| text.strip_prefix("0X"))?;
    if hex.len() != 40 || !hex.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return None;
    }
    let checksummed = eip55(&hex.to_ascii_lowercase());
    let mixed =
        hex.bytes().any(|b| b.is_ascii_lowercase()) && hex.bytes().any(|b| b.is_ascii_uppercase());
    // A mixed-case address carries an EIP-55 checksum and must match it.
    (!mixed || checksummed[2..] == *hex).then_some(checksummed)
}

/// Twelve to twenty-four plain words, a 64-hex key, a WIF key, an extended
/// private key or a 64-byte Solana keypair. Addresses never contain spaces, so
/// the word rule cannot refuse a real address.
pub fn looks_like_secret(text: &str) -> bool {
    let text = text.trim();
    let words = text.split_whitespace().collect::<Vec<_>>();
    if (12..=24).contains(&words.len())
        && words.iter().all(|word| {
            (3..=8).contains(&word.len()) && word.bytes().all(|b| b.is_ascii_alphabetic())
        })
    {
        return true;
    }
    let bare = text
        .strip_prefix("0x")
        .or_else(|| text.strip_prefix("0X"))
        .unwrap_or(text);
    if bare.len() == 64 && bare.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return true;
    }
    if ["xprv", "yprv", "zprv", "tprv", "uprv", "vprv"]
        .iter()
        .any(|prefix| text.starts_with(prefix))
    {
        return true;
    }
    if let Some(payload) = base58check(text) {
        let wif = matches!(payload.first(), Some(0x80 | 0xef))
            && (payload.len() == 33 || (payload.len() == 34 && payload[33] == 0x01));
        if wif {
            return true;
        }
    }
    if (80..=90).contains(&text.len()) && base58_decode(text).is_some_and(|bytes| bytes.len() == 64)
    {
        return true;
    }
    // A keypair file pasted as a JSON byte array.
    if text.starts_with('[') && text.ends_with(']') && text.matches(',').count() == 63 {
        return true;
    }
    false
}

pub fn check_address(text: &str) -> AddressCheck {
    if looks_like_secret(text) {
        return AddressCheck::Secret;
    }
    let text = text.trim();
    if text.is_empty()
        || text.len() > 90
        || text.chars().any(|c| c.is_whitespace() || c.is_control())
    {
        return AddressCheck::Unsupported;
    }
    if text.len() > 3 && text[..3].eq_ignore_ascii_case("bc1") {
        return segwit_mainnet(text)
            .map(|address| AddressCheck::Valid(WalletFamily::Bitcoin, address))
            .unwrap_or(AddressCheck::Unsupported);
    }
    if text.starts_with("0x") || text.starts_with("0X") {
        return evm(text)
            .map(|address| AddressCheck::Valid(WalletFamily::Evm, address))
            .unwrap_or(AddressCheck::Unsupported);
    }
    if matches!(text.as_bytes().first(), Some(b'1' | b'3')) && (26..=35).contains(&text.len()) {
        if let Some(payload) = base58check(text) {
            if payload.len() == 21 && matches!(payload[0], 0x00 | 0x05) {
                return AddressCheck::Valid(WalletFamily::Bitcoin, text.to_owned());
            }
        }
    }
    if (32..=44).contains(&text.len()) && base58_decode(text).is_some_and(|bytes| bytes.len() == 32)
    {
        return AddressCheck::Valid(WalletFamily::Solana, text.to_owned());
    }
    AddressCheck::Unsupported
}

/// First six and last four characters; the only form the renderer shows.
pub fn masked(address: &str) -> String {
    let characters = address.chars().collect::<Vec<_>>();
    if characters.len() <= 12 {
        return address.to_owned();
    }
    format!(
        "{}…{}",
        characters[..6].iter().collect::<String>(),
        characters[characters.len() - 4..]
            .iter()
            .collect::<String>()
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keccak_matches_known_digests() {
        assert_eq!(
            hex::encode(keccak256(b"")),
            "c5d2460186f7233c927e7db2dcc703c0e500b653ca82273b7bfad8045d85a470"
        );
        assert_eq!(
            hex::encode(keccak256(b"hello world")),
            "47173285a8d7341e5e972fc677286384f802f8ef42a5ec5f03bbfa254cb01fad"
        );
    }

    #[test]
    fn address_checksums_per_network() {
        for (input, family, stored) in [
            (
                "BC1QRP33G0Q5C5TXSP9ARYSRX4K6ZDKFS4NCE4XJ0GDCCCEFVPYSXF3QCCFMV3",
                WalletFamily::Bitcoin,
                "bc1qrp33g0q5c5txsp9arysrx4k6zdkfs4nce4xj0gdcccefvpysxf3qccfmv3",
            ),
            (
                "bc1qrp33g0q5c5txsp9arysrx4k6zdkfs4nce4xj0gdcccefvpysxf3qccfmv3",
                WalletFamily::Bitcoin,
                "bc1qrp33g0q5c5txsp9arysrx4k6zdkfs4nce4xj0gdcccefvpysxf3qccfmv3",
            ),
            (
                "bc1p0xlxvlhemja6c4dqv22uapctqupfhlxm9h8z3k2e72q4k9hcz7vqzk5jj0",
                WalletFamily::Bitcoin,
                "bc1p0xlxvlhemja6c4dqv22uapctqupfhlxm9h8z3k2e72q4k9hcz7vqzk5jj0",
            ),
            (
                "1A1zP1eP5QGefi2DMPTfTL5SLmv7DivfNa",
                WalletFamily::Bitcoin,
                "1A1zP1eP5QGefi2DMPTfTL5SLmv7DivfNa",
            ),
            (
                "3J98t1WpEZ73CNmQviecrnyiWrnqRhWNLy",
                WalletFamily::Bitcoin,
                "3J98t1WpEZ73CNmQviecrnyiWrnqRhWNLy",
            ),
            (
                "0x5aAeb6053F3E94C9b9A09f33669435E7Ef1BeAed",
                WalletFamily::Evm,
                "0x5aAeb6053F3E94C9b9A09f33669435E7Ef1BeAed",
            ),
            (
                "0xfb6916095ca1df60bb79ce92ce3ea74c37c5d359",
                WalletFamily::Evm,
                "0xfB6916095ca1df60bB79Ce92cE3Ea74c37c5d359",
            ),
            (
                "0xdbF03B407c01E7cD3CBea99509d93f8DDDC8C6FB",
                WalletFamily::Evm,
                "0xdbF03B407c01E7cD3CBea99509d93f8DDDC8C6FB",
            ),
            (
                "0xD1220A0cf47c7B9Be7A2E6BA89F429762e7b9aDb",
                WalletFamily::Evm,
                "0xD1220A0cf47c7B9Be7A2E6BA89F429762e7b9aDb",
            ),
            (
                "9WzDXwBbmkg8ZTbNMqUxvQRAyrZzDsGYdLVL9zYtAWWM",
                WalletFamily::Solana,
                "9WzDXwBbmkg8ZTbNMqUxvQRAyrZzDsGYdLVL9zYtAWWM",
            ),
            (
                "So11111111111111111111111111111111111111112",
                WalletFamily::Solana,
                "So11111111111111111111111111111111111111112",
            ),
        ] {
            assert_eq!(
                check_address(input),
                AddressCheck::Valid(family, stored.into()),
                "{input}"
            );
        }
        for input in [
            // A wrong bech32 checksum, a bech32m string with a v0 program, a testnet address.
            "bc1qrp33g0q5c5txsp9arysrx4k6zdkfs4nce4xj0gdcccefvpysxf3qccfmv4",
            "bc1qw508d6qejxtdg4c5r3zarvary0c5xw7kv8f3t4bc1",
            "tb1qw508d6qejxtdg4c5r3zarvary0c5xw7kxpjzsx",
            "Bc1qw508d6qejxtdg4c5r3zarvary0c5xw7kv8f3t4",
            // One base58check digit changed.
            "1A1zP1eP5QGefi2DMPTfTL5SLmv7DivfNb",
            // EIP-55 case flipped on one letter.
            "0x5aAeb6053F3E94C9b9A09f33669435E7Ef1BeAeD",
            "0x5aAeb6053F3E94C9b9A09f33669435E7Ef1BeA",
            "",
            "not an address",
            "0OIl0OIl0OIl0OIl0OIl0OIl0OIl0OIl",
        ] {
            assert_eq!(check_address(input), AddressCheck::Unsupported, "{input}");
        }
    }

    #[test]
    fn secret_guard_refuses_phrases_and_keys() {
        for secret in [
            "abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon about",
            "  Legal Winner Thank Year Wave Sausage Worth Useful Legal Winner Thank Yellow ",
            "0c28fca386c7a227600b2fe50b7cae11ec86d3bf1fbe471be89827e19d72aa1d",
            "0x0c28fca386c7a227600b2fe50b7cae11ec86d3bf1fbe471be89827e19d72aa1d",
            "5HueCGU8rMjxEXxiPuD5BDku4MkFqeZyd4dZ1jvhTVqvbTLvyTJ",
            "KwdMAjGmerYanjeui5SHS7JkmpZvVipYvB2LJGU1ZxJwYvP98617",
            "xprv9s21ZrQH143K3QTDL4LXw2F7HEK3wJUD2nW2nRk4stbPy6cq3jPPqjiChkVvvNKmPGJxWUtg6LnF5kejMRNNU3TGtRBeJgk33yuGBxrMPHi",
        ] {
            assert!(looks_like_secret(secret), "{secret}");
            assert_eq!(check_address(secret), AddressCheck::Secret);
        }
        for public in [
            "bc1qrp33g0q5c5txsp9arysrx4k6zdkfs4nce4xj0gdcccefvpysxf3qccfmv3",
            "0x5aAeb6053F3E94C9b9A09f33669435E7Ef1BeAed",
            "9WzDXwBbmkg8ZTbNMqUxvQRAyrZzDsGYdLVL9zYtAWWM",
            "short words like to a at keep a sentence from looking like a phrase",
        ] {
            assert!(!looks_like_secret(public), "{public}");
        }
    }

    #[test]
    fn masking_keeps_six_and_four() {
        assert_eq!(
            masked("bc1qxy2kgdygjrsqtzq2n0yrf2493p83kkfjhx0wlh"),
            "bc1qxy…0wlh"
        );
        assert_eq!(
            masked("0x71C7656EC7ab88b098defB751B7401B5f6d8976F"),
            "0x71C7…976F"
        );
    }
}
