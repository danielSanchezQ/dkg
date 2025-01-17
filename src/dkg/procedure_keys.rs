/// todo: eventually https://internals.rust-lang.org/t/pre-rfc-module-level-generics/12015
use crate::cryptography::elgamal::{HybridCiphertext, PublicKey, SecretKey};
use crate::dkg::committee::EncryptedShares;
use crate::traits::{PrimeGroupElement, Scalar};
use rand_core::{CryptoRng, RngCore};
use std::cmp::Ordering;

/// Committee member secret key share.
#[derive(Clone, Debug, PartialEq)]
pub struct MemberSecretShare<G: PrimeGroupElement>(pub(crate) SecretKey<G>);

/// Committee member public key share.
#[derive(Debug, Clone, Eq, PartialEq)]
pub struct MemberPublicShare<G: PrimeGroupElement>(pub(crate) PublicKey<G>);

/// Committee member communication private key. This differs from the secret share, as the members
/// need a pre-existing keypair to communicate with other members.
#[derive(Clone, Debug, PartialEq)]
pub struct MemberCommunicationKey<G: PrimeGroupElement>(pub(crate) SecretKey<G>);

/// Committee Member communication public key. This differs from the public share, as the members
/// need a pre-existing keypair to communicate with other members.
#[derive(Debug, Clone, Eq, PartialEq)]
pub struct MemberCommunicationPublicKey<G: PrimeGroupElement>(pub(crate) PublicKey<G>);

impl<G: PrimeGroupElement> Ord for MemberCommunicationPublicKey<G> {
    fn cmp(&self, other: &Self) -> Ordering {
        let self_bytes = self.0.pk.to_bytes();
        let other_bytes = other.0.pk.to_bytes();

        let mut ordering = Ordering::Equal;
        for (s, o) in self_bytes.iter().zip(other_bytes.iter()) {
            ordering = s.cmp(o);
            if ordering != Ordering::Equal {
                break;
            }
        }
        ordering
    }
}

impl<G: PrimeGroupElement> PartialOrd for MemberCommunicationPublicKey<G> {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

/// The overall committee public key used for everyone to encrypt their vote to.
#[derive(Debug, Clone, Eq, PartialEq)]
pub struct MasterPublicKey<G: PrimeGroupElement>(pub(crate) PublicKey<G>);

impl<G: PrimeGroupElement> MemberSecretShare<G> {
    pub fn to_public(&self) -> MemberPublicShare<G> {
        MemberPublicShare(PublicKey {
            pk: G::generator() * self.0.sk,
        })
    }
}

// impl<G: PrimeGroupElement> MemberPublicShare<G> {
//     pub fn to_bytes(&self) -> Vec<u8> {
//         self.0.to_bytes()
//     }
// }

impl<G: PrimeGroupElement> From<PublicKey<G>> for MemberPublicShare<G> {
    fn from(pk: PublicKey<G>) -> MemberPublicShare<G> {
        MemberPublicShare(pk)
    }
}

impl<G: PrimeGroupElement> MemberCommunicationKey<G> {
    pub fn new<R: RngCore + CryptoRng>(rng: &mut R) -> Self {
        let sk = SecretKey::generate(rng);
        MemberCommunicationKey(sk)
    }

    pub fn to_public(&self) -> MemberCommunicationPublicKey<G> {
        MemberCommunicationPublicKey(PublicKey {
            pk: G::generator() * self.0.sk,
        })
    }

    pub fn hybrid_decrypt(&self, ciphertext: &HybridCiphertext<G>) -> Vec<u8> {
        self.0.hybrid_decrypt(ciphertext)
    }

    pub(crate) fn decrypt_shares(
        &self,
        shares: EncryptedShares<G>,
    ) -> (
        Option<G::CorrespondingScalar>,
        Option<G::CorrespondingScalar>,
    ) {
        let decrypted_share = <G::CorrespondingScalar as Scalar>::from_bytes(
            &self.hybrid_decrypt(&shares.encrypted_share),
        );
        let decrypted_randomness = <G::CorrespondingScalar as Scalar>::from_bytes(
            &self.hybrid_decrypt(&shares.encrypted_randomness),
        );

        (decrypted_share, decrypted_randomness)
    }
}

impl<G: PrimeGroupElement> From<PublicKey<G>> for MemberCommunicationPublicKey<G> {
    fn from(pk: PublicKey<G>) -> MemberCommunicationPublicKey<G> {
        Self(pk)
    }
}

impl<G: PrimeGroupElement> MemberCommunicationPublicKey<G> {
    pub fn hybrid_encrypt<R>(&self, message: &[u8], rng: &mut R) -> HybridCiphertext<G>
    where
        R: RngCore + CryptoRng,
    {
        self.0.hybrid_encrypt(message, rng)
    }
}

impl<G: PrimeGroupElement> MasterPublicKey<G> {
    /// Create an election public key from all the participants of this committee
    pub fn from_participants(pks: &[MemberPublicShare<G>]) -> Self {
        let mut k = pks[0].0.pk;
        for pk in &pks[1..] {
            k = k + pk.0.pk;
        }
        MasterPublicKey(PublicKey { pk: k })
    }

    #[doc(hidden)]
    pub fn as_raw(&self) -> &PublicKey<G> {
        &self.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cryptography::elgamal::Keypair;
    use curve25519_dalek::ristretto::RistrettoPoint;
    use rand_core::OsRng;

    #[test]
    fn from_fns() {
        let mut rng = OsRng;
        let keypair = Keypair::<RistrettoPoint>::generate(&mut rng);
        let sk_comm = MemberCommunicationKey::<RistrettoPoint>(keypair.secret_key);
        let pk_comm = MemberCommunicationPublicKey::<RistrettoPoint>(keypair.public_key);

        let pk_comm_exp = sk_comm.to_public();

        assert_eq!(pk_comm, pk_comm_exp);
    }
}
