//! wallet2's `client` request member: a per-wallet key's public key, a
//! microsecond timestamp, and a signature of the timestamp.

use std::fmt;
use std::sync::Arc;

use curve25519_dalek::{constants::ED25519_BASEPOINT_TABLE, scalar::Scalar};
use rand::RngCore;
use zeroize::Zeroizing;

use crate::crypto::hash_to_scalar;

/// The key a wallet signs its daemon requests with. Never sign one wallet's
/// requests with another wallet's key: the node could link the two.
#[derive(Clone)]
pub struct RpcClientKey(Arc<Key>);

struct Key {
    secret: Zeroizing<Scalar>,
    public: [u8; 32],
}

impl fmt::Debug for RpcClientKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_tuple("RpcClientKey")
            .field(&hex::encode(self.0.public))
            .finish()
    }
}

fn random_scalar() -> Zeroizing<Scalar> {
    let mut wide = Zeroizing::new([0u8; 64]);
    rand::rngs::OsRng.fill_bytes(&mut *wide);
    Zeroizing::new(Scalar::from_bytes_mod_order_wide(&wide))
}

impl PartialEq for RpcClientKey {
    fn eq(&self, other: &Self) -> bool {
        self.0.public == other.0.public
    }
}

impl Eq for RpcClientKey {}

impl RpcClientKey {
    /// A new random key, drawn as wallet2 draws one (`rct::skGen`).
    pub fn random() -> Self {
        Self::from_scalar(random_scalar())
    }

    fn from_scalar(secret: Zeroizing<Scalar>) -> Self {
        let public = (&*secret * ED25519_BASEPOINT_TABLE).compress().to_bytes();
        RpcClientKey(Arc::new(Key { secret, public }))
    }

    /// The key [`Self::to_secret_bytes`] returned; refused unless it is a
    /// canonical nonzero scalar.
    pub fn from_secret_bytes(bytes: &[u8; 32]) -> Result<Self, String> {
        let secret = Option::<Scalar>::from(Scalar::from_canonical_bytes(*bytes))
            .filter(|secret| *secret != Scalar::ZERO)
            .ok_or_else(|| "RPC client key is not a canonical nonzero scalar".to_string())?;
        Ok(Self::from_scalar(Zeroizing::new(secret)))
    }

    /// The secret key, to store with the wallet's state.
    pub fn to_secret_bytes(&self) -> Zeroizing<[u8; 32]> {
        Zeroizing::new(self.0.secret.to_bytes())
    }

    /// The public key, which the node sees in every signed request.
    pub fn public_key(&self) -> [u8; 32] {
        self.0.public
    }

    /// The `client` value for a request sent now; empty without a clock.
    pub(crate) fn signature(&self) -> String {
        match current_time_micros() {
            Some(micros) => self.signature_at(micros),
            None => String::new(),
        }
    }

    pub(crate) fn signature_at(&self, micros: u64) -> String {
        let timestamp = format!("{micros:016x}");
        let hash = monero_oxide::primitives::keccak256(timestamp.as_bytes());
        let (c, r) = generate_signature(&hash, &self.0.public, &self.0.secret);
        let mut out = String::with_capacity(208);
        out.push_str(&hex::encode(self.0.public));
        out.push_str(&timestamp);
        out.push_str(&hex::encode(c.to_bytes()));
        out.push_str(&hex::encode(r.to_bytes()));
        out
    }
}

fn generate_signature(
    prefix_hash: &[u8; 32],
    public: &[u8; 32],
    secret: &Scalar,
) -> (Scalar, Scalar) {
    loop {
        let k = random_scalar();
        let commitment = (&*k * ED25519_BASEPOINT_TABLE).compress().to_bytes();
        let mut buf = [0u8; 96];
        buf[..32].copy_from_slice(prefix_hash);
        buf[32..64].copy_from_slice(public);
        buf[64..].copy_from_slice(&commitment);
        let c = hash_to_scalar(&buf);
        if c == Scalar::ZERO {
            continue;
        }
        let r = *k - c * secret;
        if r == Scalar::ZERO {
            continue;
        }
        return (c, r);
    }
}

fn current_time_micros() -> Option<u64> {
    #[cfg(not(target_arch = "wasm32"))]
    {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .ok()
            .and_then(|duration| u64::try_from(duration.as_micros()).ok())
    }
    #[cfg(target_arch = "wasm32")]
    {
        use rand::Rng;
        let below = rand::rngs::OsRng.gen_range(0..1000);
        micros_from_millis(js_sys::Date::now(), below, &LAST_MICROS)
    }
}

#[cfg(target_arch = "wasm32")]
static LAST_MICROS: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

/// A microsecond timestamp from a millisecond clock, with the microseconds
/// drawn uniformly as wallet2's fall, kept strictly increasing.
#[cfg_attr(not(target_arch = "wasm32"), allow(dead_code))]
fn micros_from_millis(millis: f64, below: u64, last: &std::sync::atomic::AtomicU64) -> Option<u64> {
    use std::sync::atomic::Ordering;
    if !(millis.is_finite() && millis >= 0.0 && millis < 1e15) {
        return None;
    }
    let micros = (millis.floor() as u64)
        .checked_mul(1000)?
        .checked_add(below % 1000)?;
    let mut previous = last.load(Ordering::Relaxed);
    loop {
        let next = micros.max(previous.checked_add(1)?);
        match last.compare_exchange_weak(previous, next, Ordering::Relaxed, Ordering::Relaxed) {
            Ok(_) => return Some(next),
            Err(seen) => previous = seen,
        }
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use curve25519_dalek::edwards::CompressedEdwardsY;

    pub(crate) fn verify(client: &str) -> Option<u64> {
        if client.len() != 208 {
            return None;
        }
        let public: [u8; 32] = hex::decode(&client[..64]).ok()?.try_into().ok()?;
        let timestamp = &client[64..80];
        let sig = hex::decode(&client[80..]).ok()?;
        let c = Option::<Scalar>::from(Scalar::from_canonical_bytes(sig[..32].try_into().ok()?))?;
        let r = Option::<Scalar>::from(Scalar::from_canonical_bytes(sig[32..].try_into().ok()?))?;
        let point = CompressedEdwardsY(public).decompress()?;
        let commitment = (&r * ED25519_BASEPOINT_TABLE) + c * point;
        let mut buf = [0u8; 96];
        buf[..32].copy_from_slice(&monero_oxide::primitives::keccak256(timestamp.as_bytes()));
        buf[32..64].copy_from_slice(&public);
        buf[64..].copy_from_slice(&commitment.compress().to_bytes());
        (hash_to_scalar(&buf) == c).then(|| u64::from_str_radix(timestamp, 16).ok())?
    }

    #[test]
    fn signature_has_wallet2s_layout_and_verifies() {
        let id = RpcClientKey::random();
        let client = id.signature_at(0x0006_6099_c4a3_d223);
        assert_eq!(client.len(), 208);
        assert_eq!(&client[..64], hex::encode(id.0.public));
        assert_eq!(&client[64..80], "00066099c4a3d223");
        assert_eq!(verify(&client), Some(0x0006_6099_c4a3_d223));

        let mut forged = client.clone();
        forged.replace_range(79..80, "4");
        assert_eq!(verify(&forged), None);
    }

    #[test]
    fn signature_made_by_wallet2_verifies_with_the_same_scheme() {
        let wallet2 = concat!(
            "0d8a2555c3e32d990f0c0e753053b554b4d6fb1ff901dc1b5d1f3f9fd5fdec87",
            "00066099c4a3d223",
            "0674fe5b894f7d631569f63bd031cd38bf7b63143652e12b86d92b8738b58e0d",
            "5797876c2c64be9162909559932430328c15696657224441c6e7859021cb2600",
        );
        assert_eq!(verify(wallet2), Some(0x0006_6099_c4a3_d223));
    }

    #[test]
    fn a_stored_key_signs_as_the_key_it_was_stored_from() {
        let key = RpcClientKey::random();
        let restored = RpcClientKey::from_secret_bytes(&key.to_secret_bytes()).unwrap();
        assert_eq!(restored, key);
        assert_eq!(
            &restored.signature_at(5)[..64],
            hex::encode(key.public_key())
        );
        assert_eq!(verify(&restored.signature_at(5)), Some(5));
        assert_ne!(RpcClientKey::random(), key);
        assert!(RpcClientKey::from_secret_bytes(&[0; 32]).is_err());
        assert!(RpcClientKey::from_secret_bytes(&[0xff; 32]).is_err());
        assert!(!format!("{key:?}").contains(&hex::encode(*key.to_secret_bytes())));
    }

    #[test]
    fn microseconds_from_a_millisecond_clock_look_like_wallet2s() {
        use rand::Rng;
        let last = std::sync::atomic::AtomicU64::new(0);
        let mut rng = rand::rngs::OsRng;
        let mut counts = [0u32; 10];
        let mut previous = 0;
        for step in 0..10_000u64 {
            let millis = 1_700_000_000_000.0 + (step * 3 / 2) as f64;
            let micros = micros_from_millis(millis, rng.gen_range(0..1000), &last).unwrap();
            assert!(micros > previous);
            assert!(micros / 1000 >= millis as u64 && micros / 1000 <= millis as u64 + 1);
            counts[(micros % 1000 / 100) as usize] += 1;
            previous = micros;
        }
        assert!(
            counts.iter().all(|&count| (800..1200).contains(&count)),
            "{counts:?}"
        );

        let last = std::sync::atomic::AtomicU64::new(0);
        let first = micros_from_millis(5.0, 900, &last).unwrap();
        assert_eq!(first, 5900);
        assert_eq!(micros_from_millis(5.0, 100, &last), Some(5901));
        assert_eq!(micros_from_millis(f64::NAN, 1, &last), None);
        assert_eq!(micros_from_millis(-1.0, 1, &last), None);
    }

    #[test]
    fn current_time_is_in_microseconds() {
        let micros = current_time_micros().unwrap();
        let secs = crate::wallet_state::current_time_secs().unwrap();
        assert!(micros / 1_000_000 >= secs.saturating_sub(1) && micros / 1_000_000 <= secs + 1);
    }
}
