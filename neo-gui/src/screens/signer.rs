//! Signer: the validator key-management backend (software / cloud HSM / TEE).
//!
//! This view reflects the node's signer configuration. Selecting a backend
//! shows what it requires; the node is configured via its `[hsm]` / `[tee]`
//! config sections (see the design docs).

use egui::Ui;

use crate::app::NeoGuiApp;
use crate::theme;
use crate::widgets;

struct Backend {
    name: &'static str,
    summary: &'static str,
    config: &'static str,
}

const BACKENDS: &[Backend] = &[
    Backend {
        name: "Software keystore",
        summary: "NEP-6 wallet on disk. Simplest; key material lives on the host. Fine for non-validator nodes.",
        config: "[signer]\nbackend = \"software\"\nwallet = \"wallet.json\"",
    },
    Backend {
        name: "AWS CloudHSM",
        summary: "secp256r1 key in AWS CloudHSM via PKCS#11. Key never leaves the HSM; signing is FIPS-validated.",
        config: "[hsm]\nprovider = \"aws\"\nlibrary = \"/opt/cloudhsm/lib/libcloudhsm_pkcs11.so\"\nkey_label = \"neo-validator\"",
    },
    Backend {
        name: "Azure (Managed / Dedicated HSM)",
        summary: "secp256r1 (ES256) in Azure Managed HSM (REST) or Dedicated HSM / Cloud HSM (PKCS#11).",
        config: "[hsm]\nprovider = \"azure-managed\"\nvault = \"https://my-hsm.managedhsm.azure.net\"\nkey = \"neo-validator\"",
    },
    Backend {
        name: "Google Cloud HSM",
        summary: "EC_SIGN_P256_SHA256 in Cloud KMS (HSM) via libkmsp11.so or the native API. (DER signatures are normalized.)",
        config: "[hsm]\nprovider = \"gcp\"\nlibrary = \"/usr/lib/libkmsp11.so\"\nkey = \"projects/…/cryptoKeyVersions/1\"",
    },
    Backend {
        name: "AWS Nitro Enclave (TEE)",
        summary: "Key sealed inside a Nitro Enclave; signing + attested fair ordering run in the TEE over vsock.",
        config: "[tee]\nplatform = \"nitro\"\nenclave_cid = 16\nfair_ordering = true",
    },
];

pub fn ui(app: &mut NeoGuiApp, ui: &mut Ui) {
    ui.heading("Signer & key management");
    ui.add_space(6.0);
    ui.label(
        egui::RichText::new("Where the validator signing key lives and how transactions are ordered.")
            .color(theme::TEXT_MUTED),
    );
    ui.add_space(12.0);

    ui.horizontal(|ui| {
        ui.vertical(|ui| {
            ui.set_width(240.0);
            for (i, b) in BACKENDS.iter().enumerate() {
                let selected = app.signer_backend == i;
                let label = egui::RichText::new(format!("  {}", b.name))
                    .size(14.0)
                    .color(if selected { theme::ACCENT } else { theme::TEXT });
                if ui
                    .add_sized([ui.available_width(), 36.0], egui::SelectableLabel::new(selected, label))
                    .clicked()
                {
                    app.signer_backend = i;
                }
                ui.add_space(2.0);
            }
        });
        ui.add_space(12.0);
        ui.vertical(|ui| {
            let b = &BACKENDS[app.signer_backend.min(BACKENDS.len() - 1)];
            widgets::card(ui, |ui| {
                ui.label(egui::RichText::new(b.name).size(16.0).strong().color(theme::ACCENT));
                ui.add_space(6.0);
                ui.label(egui::RichText::new(b.summary).color(theme::TEXT));
                ui.add_space(12.0);
                ui.label(egui::RichText::new("Example node config").color(theme::TEXT_MUTED).size(12.0));
                ui.add_space(4.0);
                let mut cfg = b.config.to_string();
                ui.add(
                    egui::TextEdit::multiline(&mut cfg)
                        .code_editor()
                        .desired_width(f32::INFINITY)
                        .interactive(false),
                );
            });
        });
    });

    ui.add_space(12.0);
    widgets::card(ui, |ui| {
        ui.label(
            egui::RichText::new(
                "All cloud HSM backends sign with secp256r1 (Neo's curve) and keep the key in the HSM/enclave. \
                 The Nitro backend additionally runs verifiable fair transaction ordering inside the TEE.",
            )
            .color(theme::TEXT_MUTED),
        );
    });
}
