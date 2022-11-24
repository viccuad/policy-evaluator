use anyhow::{anyhow, Result};
use kubewarden_policy_sdk::host_capabilities::verification::{
    KeylessInfo, KeylessPrefixInfo, VerificationResponse,
};
use policy_fetcher::sigstore;
use policy_fetcher::sources::Sources;
use policy_fetcher::verify::config::{LatestVerificationConfig, Signature, Subject};
use policy_fetcher::verify::{fetch_sigstore_remote_data, FulcioAndRekorData, Verifier};
use sigstore::cosign::verification_constraint::{
    AnnotationVerifier, CertificateVerifier, VerificationConstraintVec,
};
use sigstore::registry::{Certificate, CertificateEncoding};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::Mutex;
use tracing::warn;

pub(crate) struct Client {
    cosign_client: Arc<Mutex<sigstore::cosign::Client>>,
    verifier: Verifier,
}

impl Client {
    pub fn new(
        sources: Option<Sources>,
        fulcio_and_rekor_data: Option<&FulcioAndRekorData>,
    ) -> Result<Self> {
        let cosign_client = Arc::new(Mutex::new(Self::build_cosign_client(
            sources.clone(),
            fulcio_and_rekor_data,
        )?));
        let verifier = Verifier::new_from_cosign_client(cosign_client.clone(), sources);

        Ok(Client {
            cosign_client,
            verifier,
        })
    }

    fn build_cosign_client(
        sources: Option<Sources>,
        fulcio_and_rekor_data: Option<&FulcioAndRekorData>,
    ) -> Result<sigstore::cosign::Client> {
        let client_config: sigstore::registry::ClientConfig = sources.unwrap_or_default().into();
        let mut cosign_client_builder =
            sigstore::cosign::ClientBuilder::default().with_oci_client_config(client_config);
        match fulcio_and_rekor_data {
            Some(FulcioAndRekorData::FromTufRepository { repo }) => {
                cosign_client_builder = cosign_client_builder
                    .with_rekor_pub_key(repo.rekor_pub_key())
                    .with_fulcio_certs(repo.fulcio_certs());
            }
            Some(FulcioAndRekorData::FromCustomData {
                rekor_public_key,
                fulcio_certs,
            }) => {
                if let Some(pk) = rekor_public_key {
                    cosign_client_builder = cosign_client_builder.with_rekor_pub_key(pk);
                }
                if !fulcio_certs.is_empty() {
                    let certs: Vec<sigstore::registry::Certificate> = fulcio_certs
                        .iter()
                        .map(|c| {
                            let sc: sigstore::registry::Certificate = c.into();
                            sc
                        })
                        .collect();
                    cosign_client_builder = cosign_client_builder.with_fulcio_certs(&certs);
                }
            }
            None => {
                warn!("Sigstore Verifier created without Fulcio data: keyless signatures are going to be discarded because they cannot be verified");
                warn!("Sigstore Verifier created without Rekor data: transparency log data won't be used");
                warn!("Sigstore capabilities are going to be limited");
            }
        }

        cosign_client_builder = cosign_client_builder.enable_registry_caching();
        cosign_client_builder
            .build()
            .map_err(|e| anyhow!("could not build a cosign client: {}", e))
    }

    pub async fn verify_public_key(
        &mut self,
        image: String,
        pub_keys: Vec<String>,
        annotations: Option<HashMap<String, String>>,
    ) -> Result<VerificationResponse> {
        if pub_keys.is_empty() {
            return Err(anyhow!("Must provide at least one pub key"));
        }
        let mut signatures_all_of: Vec<Signature> = Vec::new();
        for k in pub_keys.iter() {
            let signature = Signature::PubKey {
                owner: None,
                key: k.clone(),
                annotations: annotations.clone(),
            };
            signatures_all_of.push(signature);
        }
        let verification_config = LatestVerificationConfig {
            all_of: Some(signatures_all_of),
            any_of: None,
        };

        let result = self.verifier.verify(&image, &verification_config).await;
        match result {
            Ok(digest) => Ok(VerificationResponse {
                digest,
                is_trusted: true,
            }),
            Err(e) => Err(e),
        }
    }

    pub async fn verify_keyless(
        &mut self,
        image: String,
        keyless: Vec<KeylessInfo>,
        annotations: Option<HashMap<String, String>>,
    ) -> Result<VerificationResponse> {
        if keyless.is_empty() {
            return Err(anyhow!("Must provide keyless info"));
        }
        // Build interim VerificationConfig:
        //
        let mut signatures_all_of: Vec<Signature> = Vec::new();
        for k in keyless.iter() {
            let signature = Signature::GenericIssuer {
                issuer: k.issuer.clone(),
                subject: Subject::Equal(k.subject.clone()),
                annotations: annotations.clone(),
            };
            signatures_all_of.push(signature);
        }
        let verification_config = LatestVerificationConfig {
            all_of: Some(signatures_all_of),
            any_of: None,
        };

        let result = self.verifier.verify(&image, &verification_config).await;
        match result {
            Ok(digest) => Ok(VerificationResponse {
                digest,
                is_trusted: true,
            }),
            Err(e) => Err(e),
        }
    }

    pub async fn verify_keyless_prefix(
        &mut self,
        image: String,
        keyless_prefix: Vec<KeylessPrefixInfo>,
        annotations: Option<HashMap<String, String>>,
    ) -> Result<VerificationResponse> {
        if keyless_prefix.is_empty() {
            return Err(anyhow!("Must provide keyless info"));
        }
        // Build interim VerificationConfig:
        //
        let mut signatures_all_of: Vec<Signature> = Vec::new();
        for k in keyless_prefix.iter() {
            let prefix = url::Url::parse(&k.url_prefix).expect("Cannot build url prefix");
            let signature = Signature::GenericIssuer {
                issuer: k.issuer.clone(),
                subject: Subject::UrlPrefix(prefix),
                annotations: annotations.clone(),
            };
            signatures_all_of.push(signature);
        }
        let verification_config = LatestVerificationConfig {
            all_of: Some(signatures_all_of),
            any_of: None,
        };

        let result = self.verifier.verify(&image, &verification_config).await;
        match result {
            Ok(digest) => Ok(VerificationResponse {
                digest,
                is_trusted: true,
            }),
            Err(e) => Err(e),
        }
    }

    pub async fn verify_github_actions(
        &mut self,
        image: String,
        owner: String,
        repo: Option<String>,
        annotations: Option<HashMap<String, String>>,
    ) -> Result<VerificationResponse> {
        if owner.is_empty() {
            return Err(anyhow!("Must provide owner info"));
        }
        // Build interim VerificationConfig:
        //
        let mut signatures_all_of: Vec<Signature> = Vec::new();
        let signature = Signature::GithubAction {
            owner: owner.clone(),
            repo: repo.clone(),
            annotations: annotations.clone(),
        };
        signatures_all_of.push(signature);
        let verification_config = LatestVerificationConfig {
            all_of: Some(signatures_all_of),
            any_of: None,
        };

        let result = self.verifier.verify(&image, &verification_config).await;
        match result {
            Ok(digest) => Ok(VerificationResponse {
                digest,
                is_trusted: true,
            }),
            Err(e) => Err(e),
        }
    }

    pub async fn verify_certificate(
        &mut self,
        image: &str,
        certificate: &[u8],
        certificate_chain: Option<&[Vec<u8>]>,
        require_rekor_bundle: bool,
        annotations: Option<HashMap<String, String>>,
    ) -> Result<VerificationResponse> {
        let (source_image_digest, trusted_layers) =
            fetch_sigstore_remote_data(&self.cosign_client, image).await?;
        let chain: Option<Vec<Certificate>> = certificate_chain.map(|certs| {
            certs
                .iter()
                .map(|cert_data| Certificate {
                    data: cert_data.to_owned(),
                    encoding: CertificateEncoding::Pem,
                })
                .collect()
        });

        let cert_verifier =
            CertificateVerifier::from_pem(certificate, require_rekor_bundle, chain.as_deref())?;

        let mut verification_constraints: VerificationConstraintVec = vec![Box::new(cert_verifier)];
        if let Some(a) = annotations {
            let annotations_verifier = AnnotationVerifier { annotations: a };
            verification_constraints.push(Box::new(annotations_verifier));
        }

        let result =
            sigstore::cosign::verify_constraints(&trusted_layers, verification_constraints.iter())
                .map(|_| source_image_digest)
                .map_err(|e| anyhow!("verification failed: {}", e));
        match result {
            Ok(digest) => Ok(VerificationResponse {
                digest,
                is_trusted: true,
            }),
            Err(e) => Err(e),
        }
    }
}
