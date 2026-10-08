use aes_gcm::{
    aead::{Aead, KeyInit},
    Aes128Gcm, Nonce,
};
use sha2::{Digest, Sha256};
use std::fmt::{Display, Formatter};

const ENC_RAND_LENGTH: usize = 16;
const AES_KEY_LENGTH: usize = 16;
const NONCE_LENGTH: usize = 12;

#[derive(Debug, PartialEq, Eq)]
pub enum CryptoError {
    InvalidEncRandLength { actual: usize },
    InvalidKey,
    AuthenticationFailed,
}

impl Display for CryptoError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidEncRandLength { actual } => {
                write!(formatter, "enc_rand must contain 16 bytes, got {actual}")
            }
            Self::InvalidKey => formatter.write_str("unable to initialize AES-128-GCM"),
            Self::AuthenticationFailed => formatter.write_str("AES-GCM authentication failed"),
        }
    }
}

impl std::error::Error for CryptoError {}

fn triple_sha256(input: &[u8]) -> [u8; 32] {
    let first = Sha256::digest(input);
    let second = Sha256::digest(first);
    let third = Sha256::digest(second);
    third.into()
}

pub fn derive_key(enc_rand: &[u8]) -> Result<[u8; AES_KEY_LENGTH], CryptoError> {
    if enc_rand.len() != ENC_RAND_LENGTH {
        return Err(CryptoError::InvalidEncRandLength {
            actual: enc_rand.len(),
        });
    }

    let digest = triple_sha256(enc_rand);
    let mut key = [0_u8; AES_KEY_LENGTH];
    key.copy_from_slice(&digest[..AES_KEY_LENGTH]);
    Ok(key)
}

pub fn derive_nonce(
    enc_rand: &[u8],
    command: u16,
    sequence: u16,
) -> Result<[u8; NONCE_LENGTH], CryptoError> {
    if enc_rand.len() != ENC_RAND_LENGTH {
        return Err(CryptoError::InvalidEncRandLength {
            actual: enc_rand.len(),
        });
    }

    let mut input = [0_u8; 20];
    input[..2].copy_from_slice(&command.to_le_bytes());
    input[2..4].copy_from_slice(&sequence.to_le_bytes());
    input[4..].copy_from_slice(enc_rand);

    let digest = triple_sha256(&input);
    let mut nonce = [0_u8; NONCE_LENGTH];
    nonce.copy_from_slice(&digest[20..]);
    Ok(nonce)
}

pub fn encrypt(
    enc_rand: &[u8],
    command: u16,
    sequence: u16,
    plaintext: &[u8],
) -> Result<Vec<u8>, CryptoError> {
    crypt(enc_rand, command, sequence, plaintext, true)
}

pub fn decrypt(
    enc_rand: &[u8],
    command: u16,
    sequence: u16,
    ciphertext_with_tag: &[u8],
) -> Result<Vec<u8>, CryptoError> {
    crypt(enc_rand, command, sequence, ciphertext_with_tag, false)
}

fn crypt(
    enc_rand: &[u8],
    command: u16,
    sequence: u16,
    input: &[u8],
    encrypting: bool,
) -> Result<Vec<u8>, CryptoError> {
    let key = derive_key(enc_rand)?;
    let nonce = derive_nonce(enc_rand, command, sequence)?;
    let cipher = Aes128Gcm::new_from_slice(&key).map_err(|_| CryptoError::InvalidKey)?;
    let nonce = Nonce::from_slice(&nonce);
    let aad = [command.to_le_bytes(), sequence.to_le_bytes()].concat();

    if encrypting {
        cipher
            .encrypt(
                nonce,
                aes_gcm::aead::Payload {
                    msg: input,
                    aad: &aad,
                },
            )
            .map_err(|_| CryptoError::AuthenticationFailed)
    } else {
        cipher
            .decrypt(
                nonce,
                aes_gcm::aead::Payload {
                    msg: input,
                    aad: &aad,
                },
            )
            .map_err(|_| CryptoError::AuthenticationFailed)
    }
}

#[cfg(test)]
mod tests {
    use super::{decrypt, derive_key, derive_nonce, encrypt, CryptoError};

    #[test]
    fn derives_protocol_key_and_nonce() {
        let enc_rand = [0x10_u8; 16];
        let key = derive_key(&enc_rand).expect("valid enc_rand");
        let nonce = derive_nonce(&enc_rand, 0xa311, 1).expect("valid enc_rand");

        assert_eq!(
            key,
            [
                0x96, 0x40, 0x2a, 0xae, 0x09, 0x16, 0x65, 0xac, 0x6c, 0xc8, 0x1f, 0xb1, 0x29, 0xcd,
                0x09, 0x0e
            ]
        );
        assert_eq!(
            nonce,
            [0xd1, 0xb0, 0xbd, 0x5d, 0x52, 0x0f, 0x5d, 0x5a, 0xbe, 0x28, 0xcb, 0xc2]
        );
    }

    #[test]
    fn encrypts_and_decrypts_with_authentication() {
        let enc_rand = [0x42_u8; 16];
        let plaintext = b"synthetic telemetry";
        let ciphertext = encrypt(&enc_rand, 0xa311, 7, plaintext).expect("encryption succeeds");

        assert_eq!(
            decrypt(&enc_rand, 0xa311, 7, &ciphertext).expect("decryption succeeds"),
            plaintext
        );
    }

    #[test]
    fn rejects_invalid_enc_rand_and_tampering() {
        assert_eq!(
            derive_key(&[0_u8; 15]),
            Err(CryptoError::InvalidEncRandLength { actual: 15 })
        );

        let enc_rand = [0x24_u8; 16];
        let mut ciphertext =
            encrypt(&enc_rand, 0xa311, 7, b"payload").expect("encryption succeeds");
        ciphertext[0] ^= 1;
        assert_eq!(
            decrypt(&enc_rand, 0xa311, 7, &ciphertext),
            Err(CryptoError::AuthenticationFailed)
        );
        assert_eq!(
            decrypt(
                &enc_rand,
                0xa311,
                8,
                &encrypt(&enc_rand, 0xa311, 7, b"payload").expect("encryption succeeds")
            ),
            Err(CryptoError::AuthenticationFailed)
        );
    }
}
