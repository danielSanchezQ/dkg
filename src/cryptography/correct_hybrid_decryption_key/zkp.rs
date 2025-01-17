//! Non-interactive Zero Knowledge proof for correct Hybrid
//! decryption key generation. We use the notation and scheme
//! presented in Figure 5 of the Treasury voting protocol spec.
//!
//! The proof is the following:
//!
//! `NIZK{(pk, C = (C1, C2), D), (sk): D = C1^sk AND pk = g^sk}`
//!
//! which is a proof of discrete log equality. We can therefore prove
//! correct decryption using a proof of discrete log equality.
use crate::cryptography::dl_equality::DleqZkp;
use crate::cryptography::elgamal::{HybridCiphertext, SymmetricKey};
use crate::dkg::procedure_keys::{MemberCommunicationKey, MemberCommunicationPublicKey};
use crate::errors::ProofError;
use crate::traits::PrimeGroupElement;
use rand_core::{CryptoRng, RngCore};

/// Proof of correct decryption.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Zkp<G: PrimeGroupElement> {
    hybrid_dec_key_proof: DleqZkp<G>,
}

impl<G> Zkp<G>
where
    G: PrimeGroupElement,
{
    /// Generate a decryption zero knowledge proof.
    pub fn generate<R>(
        c: &HybridCiphertext<G>,
        pk: &MemberCommunicationPublicKey<G>,
        symmetric_key: &SymmetricKey<G>,
        sk: &MemberCommunicationKey<G>,
        rng: &mut R,
    ) -> Self
    where
        R: CryptoRng + RngCore,
    {
        let hybrid_dec_key_proof = DleqZkp::generate(
            &G::generator(),
            &c.e1,
            &pk.0.pk,
            &symmetric_key.group_repr,
            &sk.0.sk,
            rng,
        );
        Zkp {
            hybrid_dec_key_proof,
        }
    }

    /// Verify a decryption zero knowledge proof
    pub fn verify(
        &self,
        c: &HybridCiphertext<G>,
        symmetric_key: &SymmetricKey<G>,
        pk: &MemberCommunicationPublicKey<G>,
    ) -> Result<(), ProofError> {
        self.hybrid_dec_key_proof.verify(
            &G::generator(),
            &c.e1,
            &pk.0.pk,
            &symmetric_key.group_repr,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use curve25519_dalek::ristretto::RistrettoPoint;
    use rand_core::OsRng;

    #[test]
    pub fn it_works() {
        let mut r = OsRng;

        let comm_key = MemberCommunicationKey::<RistrettoPoint>::new(&mut r);
        let comm_pkey = comm_key.to_public();

        let plaintext = [10u8; 43];
        let ciphertext = comm_pkey.hybrid_encrypt(&plaintext, &mut r);

        let decryption_key = comm_key.0.recover_symmetric_key(&ciphertext);

        let proof = Zkp::generate(&ciphertext, &comm_pkey, &decryption_key, &comm_key, &mut r);
        assert!(proof
            .verify(&ciphertext, &decryption_key, &comm_pkey)
            .is_ok())
    }
}
