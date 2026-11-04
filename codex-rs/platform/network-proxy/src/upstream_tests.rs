use super::*;
use pretty_assertions::assert_eq;
use rama_crypto::cert::CertificateAuthorityData;
use rama_crypto::cert::CertificateIdentity;
use rama_crypto::cert::CertificateSubject;
use rama_crypto::cert::LeafCertRequest;
use rama_crypto::cert::SelfSignedCaConfig;
use rama_crypto::pki_types::CertificateDer;
use rama_http::StatusCode;
use rama_net::address::Host;
use rama_tls_rustls::dep::tokio_rustls::TlsAcceptor;
use std::collections::HashMap;
use std::fs;
use std::sync::Arc;
use tempfile::tempdir;
use tokio::io::AsyncReadExt;
use tokio::io::AsyncWriteExt;
use tokio::net::TcpListener;

fn generate_ca(common_name: &str) -> CertificateAuthorityData {
    CertificateAuthorityData::generate(SelfSignedCaConfig {
        subject: CertificateSubject {
            common_name: Some(common_name.to_string()),
            ..CertificateSubject::default()
        },
        ..SelfSignedCaConfig::default()
    })
    .unwrap()
}

fn certificate_pem(der: &CertificateDer<'_>) -> String {
    use base64::Engine as _;

    let encoded = base64::engine::general_purpose::STANDARD.encode(der.as_ref());
    let mut pem = String::from("-----BEGIN CERTIFICATE-----\n");
    for chunk in encoded.as_bytes().chunks(64) {
        pem.push_str(&String::from_utf8_lossy(chunk));
        pem.push('\n');
    }
    pem.push_str("-----END CERTIFICATE-----\n");
    pem
}

#[tokio::test]
async fn mitm_upstream_client_trusts_startup_custom_ca() {
    ensure_rustls_crypto_provider();
    let temp_dir = tempdir().unwrap();
    let startup_ca_path = temp_dir.path().join("startup-ca.pem");
    let managed_ca_path = temp_dir.path().join("managed-ca.pem");
    let startup_ca = generate_ca("startup CA");
    let managed_ca = generate_ca("managed MITM CA");
    fs::write(
        &startup_ca_path,
        certificate_pem(&startup_ca.certificate_chain()[0]),
    )
    .unwrap();
    fs::write(
        &managed_ca_path,
        certificate_pem(&managed_ca.certificate_chain()[0]),
    )
    .unwrap();

    let (server_chain, server_key) = startup_ca
        .issue_leaf(LeafCertRequest::new(
            CertificateIdentity::try_from(Host::try_from("localhost").unwrap()).unwrap(),
        ))
        .unwrap();
    let mut server_config =
        rustls::ServerConfig::builder_with_protocol_versions(rustls::ALL_VERSIONS)
            .with_no_client_auth()
            .with_single_cert(server_chain, server_key)
            .unwrap();
    server_config.alpn_protocols = vec![b"http/1.1".to_vec()];

    let env = HashMap::from([(
        "SSL_CERT_FILE",
        startup_ca_path.to_string_lossy().into_owned(),
    )]);
    let roots =
        crate::certs::upstream_tls_root_store_for_cert_path(&managed_ca_path, &env).unwrap();
    let baseline_roots =
        crate::certs::upstream_tls_root_store_for_cert_path(&managed_ca_path, &HashMap::new())
            .unwrap();
    assert_eq!(roots.len(), baseline_roots.len() + 1);

    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let acceptor = TlsAcceptor::from(Arc::new(server_config));
    let server = tokio::spawn(async move {
        let (stream, _) = listener.accept().await.unwrap();
        let mut stream = acceptor.accept(stream).await.unwrap();
        let mut request = [0; 4096];
        let bytes_read = stream.read(&mut request).await.unwrap();
        assert!(bytes_read > 0);
        stream
            .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 0\r\nConnection: close\r\n\r\n")
            .await
            .unwrap();
    });

    let client =
        UpstreamClient::direct_with_allow_local_binding(/*allow_local_binding*/ true, roots);
    let response = client
        .serve(
            Request::builder()
                .uri(format!("https://localhost:{}/", address.port()))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    server.await.unwrap();
}
