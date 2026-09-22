use std::fmt::Write;

use md5::{Digest, Md5};

pub fn md5_digest(challenge: &str, secret: &str) -> String {
    let mut hasher = Md5::new();
    hasher.update(challenge.as_bytes());
    hasher.update(secret.as_bytes());
    hasher
        .finalize()
        .iter()
        .fold(String::with_capacity(32), |mut hex, byte| {
            let _ = write!(hex, "{byte:02X}");
            hex
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn matches_digest_accepted_by_packet_tracer() {
        assert_eq!(
            md5_digest("23k4SQ42tTFM6u4deW2jb1F2dZltz4t7", "probe-secret-123"),
            "3417B8057B803EBC150BC7DABA451340"
        );
    }
}
