//! Implementation of the distributed key generation (DKG)
//! procedure presented by Gennaro, Jarecki, Krawczyk and Rabin in
//! ["Secure distributed key generation for discrete-log based cryptosystems."](https://link.springer.com/article/10.1007/s00145-006-0347-3).
//! The distinction with the original protocol lies in the use of hybrid
//! encryption. We use the description and notation presented in the technical
//! [spec](https://github.com/input-output-hk/treasury-crypto/blob/master/docs/voting_protocol_spec/Treasury_voting_protocol_spec.pdf),
//! written by Dmytro Kaidalov.

use super::errors::DkgError;
use super::procedure_keys::{
    MemberCommunicationKey, MemberCommunicationPublicKey, MemberPublicShare, MemberSecretShare,
};
use crate::cryptography::{
    commitment::CommitmentKey,
    elgamal::{HybridCiphertext, PublicKey, SecretKey},
};
use crate::polynomial::Polynomial;
use crate::traits::{PrimeGroupElement, Scalar};
use rand_core::{CryptoRng, RngCore};

pub type DistributedKeyGeneration<G> = MemberState1<G>;

/// Initial state generated by a Member, corresponding to round 1.
#[derive(Clone)]
pub struct MemberState1<G: PrimeGroupElement> {
    sk_share: MemberSecretShare<G>,
    threshold: usize,
    nr_members: usize,
    owner_index: usize,
    ck: CommitmentKey<G>,
    apubs: Vec<G>,
    coeff_comms: Vec<G>,
    encrypted_shares: Vec<IndexedEncryptedShares<G>>,
}

/// State of the member corresponding to round 2.
#[derive(Clone)]
pub struct MemberState2 {
    threshold: usize,
    misbehaving_parties: Vec<MisbehavingPartiesState1>,
}

/// Type that contains the index of the receiver, and its two encrypted
/// shares.
pub(crate) type IndexedEncryptedShares<G> = (usize, HybridCiphertext<G>, HybridCiphertext<G>);

// todo: third element should be a proof of misbehaviour.
/// Type that contains misbehaving parties detected in round 1. These
/// consist of the misbehaving member's index, the error which failed,
/// and a proof of correctness of the misbehaviour claim.
type MisbehavingPartiesState1 = (usize, DkgError, usize);

/// State of the members after round 1. This structure contains the indexed encrypted
/// shares of every other participant, `indexed_shares`, and the committed coefficients
/// of the generated polynomials, `committed_coeffs`.
#[derive(Clone)]
pub struct MembersFetchedState1<G: PrimeGroupElement> {
    indexed_shares: IndexedEncryptedShares<G>,
    committed_coeffs: Vec<G>,
}

impl<G: PrimeGroupElement> MembersFetchedState1<G> {
    fn get_index(&self) -> usize {
        self.indexed_shares.0
    }
}

impl<G: PrimeGroupElement> MemberState1<G> {
    /// Generate a new member state from random. This is round 1 of the protocol. Receives as
    /// input the threshold `t`, the expected number of participants, `n`, common reference string
    /// `crs`, `committee_pks`, and the party's index `my`. Initiates a Pedersen-VSS as a dealer,
    /// and returns the committed coefficients of its polynomials, together with encryption of the
    /// shares of the other different members.
    pub fn init<R: RngCore + CryptoRng>(
        rng: &mut R,
        t: usize,
        n: usize,
        ck: &CommitmentKey<G>,
        committee_pks: &[MemberCommunicationPublicKey<G>],
        my: usize,
    ) -> MemberState1<G> {
        assert_eq!(committee_pks.len(), n);
        assert!(t > 0);
        assert!(t <= n);
        assert!(t > n / 2);
        assert!(my < n);

        let pcomm = Polynomial::<G::CorrespondingScalar>::random(rng, t);
        let pshek = Polynomial::<G::CorrespondingScalar>::random(rng, t);

        let mut apubs = Vec::with_capacity(t);
        let mut coeff_comms = Vec::with_capacity(t);

        for (ai, &bi) in pshek.get_coefficients().zip(pcomm.get_coefficients()) {
            let apub = G::generator() * ai;
            let coeff_comm = (ck.h * bi) + apub;
            apubs.push(apub);
            coeff_comms.push(coeff_comm);
        }

        let mut encrypted_shares: Vec<IndexedEncryptedShares<G>> = Vec::with_capacity(n - 1);
        #[allow(clippy::needless_range_loop)]
        for i in 0..n {
            // don't generate share for self
            if i == my {
                continue;
            } else {
                let idx = <G::CorrespondingScalar as Scalar>::from_u64((i + 1) as u64);
                let share_comm = pcomm.evaluate(&idx);
                let share_shek = pshek.evaluate(&idx);

                let pk = &committee_pks[i];

                let ecomm = pk.hybrid_encrypt(&share_comm.to_bytes(), rng);
                let eshek = pk.hybrid_encrypt(&share_shek.to_bytes(), rng);

                encrypted_shares.push((i, ecomm, eshek));
            }
        }

        MemberState1 {
            sk_share: MemberSecretShare(SecretKey {
                sk: pshek.at_zero(),
            }),
            ck: *ck,
            threshold: t,
            nr_members: n,
            owner_index: my + 1, // committee member are 1-indexed
            apubs,
            coeff_comms,
            encrypted_shares,
        }
    }

    /// Function to proceed to phase 2. It checks and keeps track of misbehaving parties. If this
    /// step does not validate, the member is not allowed to proceed to phase 3.
    pub fn to_phase_2(
        &self,
        secret_key: &MemberCommunicationKey<G>,
        members_state: &[MembersFetchedState1<G>],
    ) -> MemberState2 {
        let mut misbehaving_parties: Vec<MisbehavingPartiesState1> = Vec::new();
        for fetched_data in members_state {
            if let (Some(comm), Some(shek)) =
                secret_key.decrypt_shares(fetched_data.indexed_shares.clone())
            {
                let index_pow =
                    <G::CorrespondingScalar as Scalar>::from_u64(self.owner_index as u64)
                        .exp_iter()
                        .take(self.threshold + 1);

                let check_element = self.ck.h * comm + G::generator() * shek;
                let multi_scalar = G::vartime_multiscalar_multiplication(
                    index_pow,
                    fetched_data.committed_coeffs.clone(),
                );

                if check_element != multi_scalar {
                    // todo: should we instead store the sender's index?
                    misbehaving_parties.push((
                        fetched_data.get_index(),
                        DkgError::ShareValidityFailed,
                        0,
                    ));
                }
            } else {
                // todo: handle the proofs. Might not be the most optimal way of handling these two
                misbehaving_parties.push((
                    fetched_data.get_index(),
                    DkgError::ScalarOutOfBounds,
                    0,
                ));
            }
        }

        MemberState2 {
            misbehaving_parties,
            threshold: self.threshold,
        }
    }

    pub fn secret_key(&self) -> &MemberSecretShare<G> {
        &self.sk_share
    }

    pub fn public_key(&self) -> MemberPublicShare<G> {
        MemberPublicShare(PublicKey { pk: self.apubs[0] })
    }
}

impl MemberState2 {
    pub fn validate(&self) -> Result<Self, DkgError> {
        if self.misbehaving_parties.len() == self.threshold {
            return Err(DkgError::MisbehaviourHigherThreshold);
        }

        Ok(self.clone())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use curve25519_dalek::ristretto::RistrettoPoint;
    use rand_core::OsRng;

    #[test]
    fn valid_phase_2() {
        let mut rng = OsRng;

        let mut shared_string = b"Example of a shared string.".to_owned();
        let h = CommitmentKey::<RistrettoPoint>::generate(&mut shared_string);

        let mc1 = MemberCommunicationKey::<RistrettoPoint>::new(&mut rng);
        let mc2 = MemberCommunicationKey::<RistrettoPoint>::new(&mut rng);
        let mc = [mc1.to_public(), mc2.to_public()];

        let threshold = 2;
        let nr_members = 2;

        let m1 = DistributedKeyGeneration::<RistrettoPoint>::init(
            &mut rng, threshold, nr_members, &h, &mc, 0,
        );
        let m2 = DistributedKeyGeneration::<RistrettoPoint>::init(
            &mut rng, threshold, nr_members, &h, &mc, 1,
        );

        // Now, party one fetches the state of the other parties, mainly party two and three
        let fetched_state = vec![MembersFetchedState1 {
            indexed_shares: m2.encrypted_shares[0].clone(),
            committed_coeffs: m2.coeff_comms.clone(),
        }];

        let phase_2 = m1.to_phase_2(&mc1, &fetched_state);

        assert!(phase_2.validate().is_ok());
    }

    #[test]
    fn invalid_phase_2() {
        let mut rng = OsRng;

        let mut shared_string = b"Example of a shared string.".to_owned();
        let h = CommitmentKey::<RistrettoPoint>::generate(&mut shared_string);

        let mc1 = MemberCommunicationKey::<RistrettoPoint>::new(&mut rng);
        let mc2 = MemberCommunicationKey::<RistrettoPoint>::new(&mut rng);
        let mc3 = MemberCommunicationKey::<RistrettoPoint>::new(&mut rng);
        let mc = [mc1.to_public(), mc2.to_public(), mc3.to_public()];

        let threshold = 2;
        let nr_members = 3;

        let m1 = DistributedKeyGeneration::<RistrettoPoint>::init(
            &mut rng, threshold, nr_members, &h, &mc, 0,
        );
        let m2 = DistributedKeyGeneration::<RistrettoPoint>::init(
            &mut rng, threshold, nr_members, &h, &mc, 1,
        );
        let m3 = DistributedKeyGeneration::<RistrettoPoint>::init(
            &mut rng, threshold, nr_members, &h, &mc, 2,
        );

        // Now, party one fetches invalid state of the other parties, mainly party two and three
        let fetched_state = vec![
            MembersFetchedState1 {
                indexed_shares: m2.encrypted_shares[0].clone(),
                committed_coeffs: vec![PrimeGroupElement::zero(); 3],
            },
            MembersFetchedState1 {
                indexed_shares: m3.encrypted_shares[0].clone(),
                committed_coeffs: vec![PrimeGroupElement::zero(); 3],
            },
        ];

        let phase_2_faked = m1.to_phase_2(&mc1, &fetched_state);
        assert!(phase_2_faked.validate().is_err());
    }
}
