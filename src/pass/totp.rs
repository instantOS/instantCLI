//! Local TOTP generation, including Steam Guard's non-standard encoder.
//!
//! `pass otp` shells out to `oathtool`, which only supports 6/7/8-digit
//! standard TOTP. Steam Guard tokens use a 5-digit code over a custom base-26
//! alphabet (`encoder=steam` in the otpauth URI), which oathtool rejects. This
//! module parses an `otpauth://` URI and generates the code ourselves so those
//! entries work without any external OTP helper.

use anyhow::{Result, anyhow, bail};
use hmac::{Hmac, Mac};
use sha1::Sha1;

const STEAM_ALPHABET: &[u8] = b"23456789BCDFGHJKMNPQRTVWXY";

/// A parsed `otpauth://totp/...` URI.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct OtpAuth {
    secret: Vec<u8>,
    digits: u32,
    period: u64,
    steam: bool,
}

impl OtpAuth {
    /// Parse an `otpauth://totp/...` URI. Only the TOTP variant is supported.
    pub(super) fn parse(uri: &str) -> Result<Self> {
        let uri = uri.trim();
        let rest = uri
            .strip_prefix("otpauth://totp/")
            .ok_or_else(|| anyhow!("Not a TOTP otpauth URI"))?;

        let query = rest.split_once('?').map(|(_, q)| q).unwrap_or("");

        let mut secret: Option<Vec<u8>> = None;
        let mut digits: u32 = 6;
        let mut period: u64 = 30;
        let mut steam = false;

        for pair in query.split('&') {
            if pair.is_empty() {
                continue;
            }
            let (key, value) = pair.split_once('=').unwrap_or((pair, ""));
            let value = percent_decode(value);
            match key.to_ascii_lowercase().as_str() {
                "secret" => secret = Some(base32_decode(&value)?),
                "digits" => {
                    digits = value
                        .parse()
                        .map_err(|_| anyhow!("Invalid digits value in otpauth URI: {value}"))?
                }
                "period" => {
                    period = value
                        .parse()
                        .map_err(|_| anyhow!("Invalid period value in otpauth URI: {value}"))?
                }
                "encoder" => steam = value.eq_ignore_ascii_case("steam"),
                "issuer" if value.eq_ignore_ascii_case("steam") => steam = true,
                _ => {}
            }
        }

        let secret = secret.ok_or_else(|| anyhow!("otpauth URI is missing a secret"))?;
        if secret.is_empty() {
            bail!("otpauth URI has an empty secret");
        }
        if period == 0 {
            bail!("otpauth URI has a zero period");
        }

        Ok(OtpAuth {
            secret,
            digits,
            period,
            steam,
        })
    }

    /// Whether `oathtool`/`pass otp` can handle this entry. Steam-encoded and
    /// non-6/7/8-digit tokens must be generated locally instead.
    pub(super) fn oathtool_supported(&self) -> bool {
        !self.steam && matches!(self.digits, 6..=8)
    }

    /// Generate the current code using the system clock.
    pub(super) fn generate(&self) -> Result<String> {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_err(|_| anyhow!("System clock is before the Unix epoch"))?
            .as_secs();
        Ok(self.generate_at(now))
    }

    fn generate_at(&self, unix_time: u64) -> String {
        let counter = unix_time / self.period;
        let hmac = hmac_sha1(&self.secret, &counter.to_be_bytes());

        // Dynamic truncation (RFC 4226).
        let offset = (hmac[hmac.len() - 1] & 0x0f) as usize;
        let full = (u32::from(hmac[offset] & 0x7f) << 24)
            | (u32::from(hmac[offset + 1]) << 16)
            | (u32::from(hmac[offset + 2]) << 8)
            | u32::from(hmac[offset + 3]);

        if self.steam {
            steam_encode(full)
        } else {
            let modulo = 10u32.pow(self.digits);
            format!("{:0width$}", full % modulo, width = self.digits as usize)
        }
    }
}

fn steam_encode(mut full: u32) -> String {
    let mut code = String::with_capacity(5);
    for _ in 0..5 {
        let index = (full % STEAM_ALPHABET.len() as u32) as usize;
        code.push(STEAM_ALPHABET[index] as char);
        full /= STEAM_ALPHABET.len() as u32;
    }
    code
}

fn hmac_sha1(key: &[u8], data: &[u8]) -> Vec<u8> {
    let mut mac = Hmac::<Sha1>::new_from_slice(key).expect("HMAC accepts keys of any size");
    mac.update(data);
    mac.finalize().into_bytes().to_vec()
}

/// Decode RFC 4648 base32 (no padding required, case-insensitive).
fn base32_decode(input: &str) -> Result<Vec<u8>> {
    let mut buffer: u32 = 0;
    let mut bits: u32 = 0;
    let mut output = Vec::new();

    for ch in input.chars() {
        if ch == '=' || ch == ' ' || ch == '-' {
            continue;
        }
        let value = match ch {
            'A'..='Z' => ch as u32 - 'A' as u32,
            'a'..='z' => ch as u32 - 'a' as u32,
            '2'..='7' => ch as u32 - '2' as u32 + 26,
            _ => bail!("Invalid base32 character in secret: {ch:?}"),
        };
        buffer = (buffer << 5) | value;
        bits += 5;
        if bits >= 8 {
            bits -= 8;
            output.push((buffer >> bits) as u8);
        }
    }

    Ok(output)
}

/// Minimal percent-decoding for otpauth query values.
fn percent_decode(input: &str) -> String {
    let bytes = input.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            let hi = (bytes[i + 1] as char).to_digit(16);
            let lo = (bytes[i + 2] as char).to_digit(16);
            if let (Some(hi), Some(lo)) = (hi, lo) {
                out.push((hi * 16 + lo) as u8);
                i += 3;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    // RFC 6238 test secret: ASCII "12345678901234567890" in base32.
    const RFC_SECRET: &str = "GEZDGNBVGY3TQOJQGEZDGNBVGY3TQOJQ";

    #[test]
    fn rfc6238_standard_totp() {
        let auth = OtpAuth::parse(&format!(
            "otpauth://totp/test?secret={RFC_SECRET}&digits=8&period=30"
        ))
        .unwrap();
        assert_eq!(auth.generate_at(59), "94287082");
        assert_eq!(auth.generate_at(1111111109), "07081804");
    }

    #[test]
    fn rfc6238_six_digits() {
        let auth = OtpAuth::parse(&format!("otpauth://totp/test?secret={RFC_SECRET}")).unwrap();
        assert_eq!(auth.digits, 6);
        assert_eq!(auth.generate_at(59), "287082");
    }

    #[test]
    fn steam_encoder_generates_base26_code() {
        let auth = OtpAuth::parse(&format!(
            "otpauth://totp/steam:user?secret={RFC_SECRET}&period=30&digits=5&issuer=steam&encoder=steam"
        ))
        .unwrap();
        assert!(auth.steam);
        assert!(!auth.oathtool_supported());
        // Cross-checked against an independent Python implementation of the
        // Steam Guard algorithm.
        assert_eq!(auth.generate_at(1111111111), "5PP3V");
        assert_eq!(auth.generate_at(0), "GG5F5");
    }

    #[test]
    fn steam_issuer_without_encoder_is_still_steam() {
        let auth = OtpAuth::parse(&format!(
            "otpauth://totp/Steam:user?secret={RFC_SECRET}&issuer=Steam"
        ))
        .unwrap();
        assert!(auth.steam);
        assert!(!auth.oathtool_supported());
    }

    #[test]
    fn standard_entry_is_oathtool_supported() {
        let auth = OtpAuth::parse(&format!("otpauth://totp/test?secret={RFC_SECRET}")).unwrap();
        assert!(auth.oathtool_supported());
    }

    #[test]
    fn base32_decodes_lowercase_and_spaces() {
        assert_eq!(base32_decode("gezd gnbv").unwrap(), b"12345");
    }

    #[test]
    fn rejects_non_totp_uri() {
        assert!(OtpAuth::parse("otpauth://hotp/test?secret=AAAA").is_err());
    }
}
