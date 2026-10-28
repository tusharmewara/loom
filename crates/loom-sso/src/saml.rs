//! SAML SP (Service Provider) for the control plane.

use anyhow::{Context, Result};
use base64::{engine::general_purpose::STANDARD as BASE64, Engine};
use std::fs;
use x509_parser::prelude::*;
use xmlsec::{crypto::XmlSecCrypto, signature::SignatureVerification};

use crate::config::{SamlConfig, UserInfo};

pub struct SamlSp {
    config: SamlConfig,
    sp_private_key: String,
    sp_certificate: String,
}

impl SamlSp {
    pub fn new(config: SamlConfig, sp_private_key: String, sp_certificate: String) -> Result<Self> {
        // Validate IdP certificate
        let _ = X509Certificate::from_der(&BASE64.decode(&config.x509_cert)?)?;
        Ok(Self {
            config,
            sp_private_key,
            sp_certificate,
        })
    }

    /// Generate SAML AuthnRequest (Redirect Binding)
    pub fn auth_request(&self, relay_state: Option<String>) -> Result<String> {
        let request_id = format!("_{}", uuid::Uuid::new_v4().simple());
        let issue_instant = chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true);
        let acs_url = format!("{}/api/v1/saml/acs", "http://127.0.0.1:7333"); // TODO: configurable

        let mut xml = String::new();
        xml.push_str(r#"<samlp:AuthnRequest xmlns:samlp="urn:oasis:names:tc:SAML:2.0:protocol""#);
        xml.push_str(r#" xmlns:saml="urn:oasis:names:tc:SAML:2.0:assertion""#);
        xml.push_str(&format!(r#" ID="{}" Version="2.0" IssueInstant="{}" ProtocolBinding="urn:oasis:names:tc:SAML:2.0:bindings:HTTP-POST" AssertionConsumerServiceURL="{}">"#, request_id, issue_instant, acs_url));
        xml.push_str(&format!(r#"<saml:Issuer>{}</saml:Issuer>"#, xml_escape(&self.config.entity_id)));
        xml.push_str(r#"<samlp:NameIDPolicy Format="urn:oasis:names:tc:SAML:1.1:nameid-format:emailAddress" AllowCreate="true"/>"#);
        xml.push_str(r#"<samlp:RequestedAuthnContext Comparison="exact"><saml:AuthnContextClassRef>urn:oasis:names:tc:SAML:2.0:ac:classes:PasswordProtectedTransport</saml:AuthnContextClassRef></samlp:RequestedAuthnContext>"#);
        xml.push_str("</samlp:AuthnRequest>");

        let compressed = compress_deflate(&xml)?;
        let encoded = BASE64.encode(compressed);
        let mut redirect_url = format!("{}?SAMLRequest={}", self.config.sso_url, urlencoding::encode(&encoded));
        if let Some(rs) = relay_state {
            redirect_url.push_str(&format!("&RelayState={}", urlencoding::encode(&rs)));
        }
        Ok(redirect_url)
    }

    /// Parse and validate SAML Assertion from ACS POST
    pub fn parse_assertion(&self, saml_response: &str) -> Result<UserInfo> {
        let decoded = BASE64.decode(saml_response)?;
        let decompressed = decompress_deflate(&decoded)?;
        let doc = xmltree::Element::parse(decompressed.as_slice())?;

        // Find Assertion
        let assertion = doc
            .get_child("Assertion")
            .or_else(|| doc.get_child("{urn:oasis:names:tc:SAML:2.0:assertion}Assertion"))
            .context("no Assertion in response")?;

        // Validate signature
        validate_signature(&assertion, &self.config.x509_cert)?;

        // Extract attributes
        let mut email = None;
        let mut name = None;
        let mut groups = Vec::new();

        for attr_stmt in assertion.children.iter().filter(|c| c.name.ends_with("AttributeStatement")) {
            for attr in attr_stmt.children.iter().filter(|c| c.name.ends_with("Attribute")) {
                let attr_name = attr.attributes.get("Name").cloned().unwrap_or_default();
                let value = attr.get_child("AttributeValue")
                    .and_then(|v| v.get_text())
                    .unwrap_or_default();

                match attr_name.as_str() {
                    "email" | "Email" | "mail" => email = Some(value),
                    "name" | "Name" | "displayName" => name = Some(value),
                    "groups" | "Groups" | "roles" => groups.push(value),
                    _ => {}
                }
            }
        }

        Ok(UserInfo {
            email: email.context("email attribute missing")?,
            name: name.unwrap_or_else(|| email.clone().unwrap_or_default()),
            groups,
        })
    }

    /// Generate SP metadata XML
    pub fn metadata(&self) -> String {
        let entity_id = xml_escape(&self.config.entity_id);
        let acs_url = format!("{}/api/v1/saml/acs", "http://127.0.0.1:7333");

        format!(
            r#"<EntityDescriptor xmlns="urn:oasis:names:tc:SAML:2.0:metadata" entityID="{}">
  <SPSSODescriptor protocolSupportEnumeration="urn:oasis:names:tc:SAML:2.0:protocol" AuthnRequestsSigned="false" WantAssertionsSigned="true">
    <KeyDescriptor use="signing">
      <KeyInfo xmlns="http://www.w3.org/2000/09/xmldsig#">
        <X509Data><X509Certificate>{}</X509Certificate>
      </KeyInfo>
    </KeyDescriptor>
    <AssertionConsumerService Binding="urn:oasis:names:tc:SAML:2.0:bindings:HTTP-POST" Location="{}" index="0" isDefault="true"/>
  </SPSSODescriptor>
</EntityDescriptor>"#,
            entity_id, self.sp_certificate, acs_url
        )
    }
}

fn validate_signature(assertion: &xmltree::Element, idp_cert_b64: &str) -> Result<()> {
    // Use xmlsec to verify the signature
    let crypto = XmlSecCrypto::new();
    let cert_der = BASE64.decode(idp_cert_b64)?;
    let verification = SignatureVerification::new(&crypto, &cert_der);
    let assertion_xml = assertion.to_string();
    verification.verify(&assertion_xml).context("SAML signature validation failed")?;
    Ok(())
}

fn compress_deflate(data: &str) -> Result<Vec<u8>> {
    use flate2::write::ZlibEncoder;
    use flate2::Compression;
    use std::io::Write;
    let mut encoder = ZlibEncoder::new(Vec::new(), Compression::default());
    encoder.write_all(data.as_bytes())?;
    Ok(encoder.finish()?)
}

fn decompress_deflate(data: &[u8]) -> Result<Vec<u8>> {
    use flate2::read::ZlibDecoder;
    use std::io::Read;
    let mut decoder = ZlibDecoder::new(data);
    let mut out = Vec::new();
    decoder.read_to_end(&mut out)?;
    Ok(out)
}

fn xml_escape(s: &str) -> String {
    s.replace('&', "&").replace('<', "<").replace('>', ">").replace('"', """).replace('\'', "&apos;")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn saml_metadata_generation() {
        let config = SamlConfig {
            entity_id: "https://sp.example.com".into(),
            sso_url: "https://idp.example.com/sso".into(),
            slo_url: None,
            x509_cert: "MIIC...".into(),
            attribute_map: Default::default(),
        };
        let sp = SamlSp::new(config, "key".into(), "cert".into()).unwrap();
        let meta = sp.metadata();
        assert!(meta.contains("https://sp.example.com"));
        assert!(meta.contains("AssertionConsumerService"));
    }

    #[test]
    fn xml_escape_test() {
        assert_eq!(xml_escape("<test>&\"'"), "<test>&"&apos;");
    }
}