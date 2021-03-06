// Copyright 2015-2017 Brian Smith.
//
// Permission to use, copy, modify, and/or distribute this software for any
// purpose with or without fee is hereby granted, provided that the above
// copyright notice and this permission notice appear in all copies.
//
// THE SOFTWARE IS PROVIDED "AS IS" AND THE AUTHORS DISCLAIM ALL WARRANTIES
// WITH REGARD TO THIS SOFTWARE INCLUDING ALL IMPLIED WARRANTIES OF
// MERCHANTABILITY AND FITNESS. IN NO EVENT SHALL THE AUTHORS BE LIABLE FOR ANY
// SPECIAL, DIRECT, INDIRECT, OR CONSEQUENTIAL DAMAGES OR ANY DAMAGES
// WHATSOEVER RESULTING FROM LOSS OF USE, DATA OR PROFITS, WHETHER IN AN ACTION
// OF CONTRACT, NEGLIGENCE OR OTHER TORTIOUS ACTION, ARISING OUT OF OR IN
// CONNECTION WITH THE USE OR PERFORMANCE OF THIS SOFTWARE.

#![forbid(
    anonymous_parameters,
    box_pointers,
    legacy_directory_ownership,
    missing_copy_implementations,
    missing_debug_implementations,
    missing_docs,
    trivial_casts,
    trivial_numeric_casts,
    unsafe_code,
    unstable_features,
    unused_extern_crates,
    unused_import_braces,
    unused_qualifications,
    unused_results,
    variant_size_differences,
    warnings
)]

use ring::{agreement::*, error, rand, test};

#[test]
fn agreement_agree() {
    agreement_agree_(
        |private_key: PrivateKey<Ephemeral>, peer_alg, peer_public_key| {
            (None, private_key.agree(peer_alg, peer_public_key))
        },
    );
    agreement_agree_(|private_key, peer_alg, peer_public_key| {
        (
            Some(private_key.agree_static(peer_alg, peer_public_key)),
            private_key.agree(peer_alg, peer_public_key),
        )
    });
}

fn agreement_agree_<U: Lifetime, F>(agree: F)
where
    F: Fn(
        PrivateKey<U>,
        &Algorithm,
        untrusted::Input,
    ) -> (
        Option<Result<InputKeyMaterial, error::Unspecified>>,
        Result<InputKeyMaterial, error::Unspecified>,
    ),
{
    let rng = rand::SystemRandom::new();

    test::from_file("tests/agreement_tests.txt", |section, test_case| {
        assert_eq!(section, "");

        let curve_name = test_case.consume_string("Curve");
        let alg = alg_from_curve_name(&curve_name);
        let peer_public = test_case.consume_bytes("PeerQ");
        let peer_public = untrusted::Input::from(&peer_public);

        match test_case.consume_optional_string("Error") {
            None => {
                let my_private = test_case.consume_bytes("D");
                let rng = test::rand::FixedSliceRandom { bytes: &my_private };
                let my_private = PrivateKey::<U>::generate(alg, &rng)?;

                let my_public = test_case.consume_bytes("MyQ");
                let output = test_case.consume_bytes("Output");

                let computed_public = my_private.compute_public_key().unwrap();
                assert_eq!(computed_public.as_ref(), &my_public[..]);

                let (static_ikm, ephemeral_ikm) = agree(my_private, alg, peer_public);
                if let Some(ikm) = static_ikm {
                    ikm.unwrap().derive(|ikm| assert_eq!(ikm, &output[..]));
                };
                ephemeral_ikm
                    .unwrap()
                    .derive(|key_material| assert_eq!(key_material, &output[..]));
            },

            Some(_) => {
                let dummy_private_key = PrivateKey::<U>::generate(alg, &rng)?;
                let (static_ikm, ephemeral_ikm) = agree(dummy_private_key, alg, peer_public);
                if let Some(ikm) = static_ikm {
                    assert!(ikm.is_err());
                };
                assert!(ephemeral_ikm.is_err());
            },
        }

        return Ok(());
    });
}

#[test]
fn test_agreement_ecdh_x25519_rfc_iterated() {
    let mut k = h("0900000000000000000000000000000000000000000000000000000000000000");
    let mut u = k.clone();

    fn expect_iterated_x25519(
        expected_result: &str, range: std::ops::Range<usize>, k: &mut Vec<u8>, u: &mut Vec<u8>,
    ) {
        for _ in range {
            let new_k = x25519(k, u);
            *u = k.clone();
            *k = new_k;
        }
        assert_eq!(&h(expected_result), k);
    }

    expect_iterated_x25519(
        "422c8e7a6227d7bca1350b3e2bb7279f7897b87bb6854b783c60e80311ae3079",
        0..1,
        &mut k,
        &mut u,
    );
    expect_iterated_x25519(
        "684cf59ba83309552800ef566f2f4d3c1c3887c49360e3875f2eb94d99532c51",
        1..1_000,
        &mut k,
        &mut u,
    );

    // The spec gives a test vector for 1,000,000 iterations but it takes
    // too long to do 1,000,000 iterations by default right now. This
    // 10,000 iteration vector is self-computed.
    expect_iterated_x25519(
        "2c125a20f639d504a7703d2e223c79a79de48c4ee8c23379aa19a62ecd211815",
        1_000..10_000,
        &mut k,
        &mut u,
    );

    if cfg!(feature = "slow_tests") {
        expect_iterated_x25519(
            "7c3911e0ab2586fd864497297e575e6f3bc601c0883c30df5f4dd2d24f665424",
            10_000..1_000_000,
            &mut k,
            &mut u,
        );
    }
}

fn x25519(private_key: &[u8], public_key: &[u8]) -> Vec<u8> {
    x25519_(private_key, public_key).unwrap()
}

fn x25519_(private_key: &[u8], public_key: &[u8]) -> Result<Vec<u8>, error::Unspecified> {
    let rng = test::rand::FixedSliceRandom { bytes: private_key };
    let private_key = PrivateKey::<Ephemeral>::generate(&X25519, &rng)?;
    let public_key = untrusted::Input::from(public_key);
    let ikm = private_key.agree(&X25519, public_key)?;
    ikm.derive(|agreed_value| Ok(Vec::from(agreed_value)))
}

fn h(s: &str) -> Vec<u8> {
    match test::from_hex(s) {
        Ok(v) => v,
        Err(msg) => {
            panic!("{} in {}", msg, s);
        },
    }
}

fn alg_from_curve_name(curve_name: &str) -> &'static Algorithm {
    if curve_name == "P-256" {
        &ECDH_P256
    } else if curve_name == "P-384" {
        &ECDH_P384
    } else if curve_name == "X25519" {
        &X25519
    } else {
        panic!("Unsupported curve: {}", curve_name);
    }
}
