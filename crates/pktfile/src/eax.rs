//! EAX (Bellare, Rogaway, Wagner) over Twofish with a 128-bit key.
//!
//! The `RustCrypto` `eax` crate stores keys at the cipher's nominal size, 256 bits
//! for Twofish, so it cannot express the 128-bit key Packet Tracer uses. This
//! builds the same construction from CMAC and CTR around a Twofish instance
//! created from the short key.

use cipher::{InnerIvInit, KeyInit, StreamCipher, StreamCipherCoreWrapper};
use cmac::{
    Cmac, CmacCore, Mac,
    digest::{InnerInit, core_api::CoreWrapper},
};
use ctr::{CtrCore, flavors::Ctr128BE};
use twofish::Twofish;

const BLOCK: usize = 16;

pub(crate) fn seal(key: &[u8; BLOCK], nonce: &[u8; BLOCK], data: &mut [u8]) -> [u8; BLOCK] {
    let cipher = Twofish::new_from_slice(key).expect("Twofish accepts 128-bit keys");
    let nonce_mac = omac(&cipher, 0, nonce);
    let header_mac = omac(&cipher, 1, &[]);
    keystream(&cipher, &nonce_mac, data);
    let data_mac = omac(&cipher, 2, data);
    xor3(&nonce_mac, &header_mac, &data_mac)
}

pub(crate) fn open(
    key: &[u8; BLOCK],
    nonce: &[u8; BLOCK],
    data: &mut [u8],
    tag: &[u8; BLOCK],
) -> bool {
    let cipher = Twofish::new_from_slice(key).expect("Twofish accepts 128-bit keys");
    let nonce_mac = omac(&cipher, 0, nonce);
    let header_mac = omac(&cipher, 1, &[]);
    let data_mac = omac(&cipher, 2, data);
    let expected = xor3(&nonce_mac, &header_mac, &data_mac);
    let matches = expected
        .iter()
        .zip(tag)
        .fold(0, |difference, (left, right)| difference | (left ^ right))
        == 0;
    if matches {
        keystream(&cipher, &nonce_mac, data);
    }
    matches
}

fn omac(cipher: &Twofish, domain: u8, data: &[u8]) -> [u8; BLOCK] {
    let mut mac: Cmac<Twofish> = CoreWrapper::from_core(CmacCore::inner_init(cipher.clone()));
    let mut prefix = [0; BLOCK];
    prefix[BLOCK - 1] = domain;
    mac.update(&prefix);
    mac.update(data);
    mac.finalize().into_bytes().into()
}

fn keystream(cipher: &Twofish, counter: &[u8; BLOCK], data: &mut [u8]) {
    let mut ctr = StreamCipherCoreWrapper::from_core(CtrCore::<Twofish, Ctr128BE>::inner_iv_init(
        cipher.clone(),
        counter.into(),
    ));
    ctr.apply_keystream(data);
}

fn xor3(a: &[u8; BLOCK], b: &[u8; BLOCK], c: &[u8; BLOCK]) -> [u8; BLOCK] {
    std::array::from_fn(|index| a[index] ^ b[index] ^ c[index])
}
