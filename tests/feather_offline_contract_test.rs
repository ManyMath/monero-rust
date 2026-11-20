use std::{env, fs, path::Path};

use monero_rust::{
    epee_compat,
    ur_codec::{
        UrEncoder, UR_TYPE_KEY_IMAGE, UR_TYPE_OUTPUT, UR_TYPE_SIGNED_TX, UR_TYPE_UNSIGNED_TX,
    },
};

fn vector_path(name: &str) -> String {
    format!(
        "{}/tests/vectors/cold_signing_regtest_v0_18_5_0/{name}",
        env!("CARGO_MANIFEST_DIR")
    )
}

fn read_vector(name: &str) -> Vec<u8> {
    fs::read(vector_path(name)).unwrap_or_else(|e| panic!("{name} fixture should read: {e}"))
}

fn assert_ur_prefix(data: &[u8], ur_type: &'static str) {
    let mut encoder = UrEncoder::new(data, ur_type, 250).expect("UR encoder should build");
    let frame = encoder.next_frame().expect("UR frame should encode");
    assert!(
        frame.uri.starts_with(&format!("ur:{ur_type}/")),
        "unexpected UR prefix for {ur_type}: {}",
        frame.uri
    );
}

fn assert_source_contains(feather_src: &Path, relative: &str, needles: &[&str]) {
    let path = feather_src.join(relative);
    let source =
        fs::read_to_string(&path).unwrap_or_else(|e| panic!("{} should read: {e}", path.display()));
    for needle in needles {
        assert!(
            source.contains(needle),
            "{} should contain {needle:?}",
            path.display()
        );
    }
}

#[test]
fn feather_wallet2_file_and_ur_contract_matches_generated_vectors() {
    let unsigned = read_vector("unsigned_monero_tx");
    let signed = read_vector("signed_monero_tx");
    let outputs = read_vector("outputs");

    assert!(unsigned.starts_with(epee_compat::UNSIGNED_TX_MAGIC));
    assert!(signed.starts_with(epee_compat::SIGNED_TX_MAGIC));
    assert!(outputs.starts_with(epee_compat::OUTPUT_EXPORT_MAGIC));

    assert_eq!(UR_TYPE_UNSIGNED_TX, "xmr-txunsigned");
    assert_eq!(UR_TYPE_SIGNED_TX, "xmr-txsigned");
    assert_eq!(UR_TYPE_OUTPUT, "xmr-output");
    assert_eq!(UR_TYPE_KEY_IMAGE, "xmr-keyimage");

    assert_ur_prefix(&unsigned, UR_TYPE_UNSIGNED_TX);
    assert_ur_prefix(&signed, UR_TYPE_SIGNED_TX);
    assert_ur_prefix(&outputs, UR_TYPE_OUTPUT);

    let Ok(feather_src) = env::var("FEATHER_SRC") else {
        eprintln!("Skipped Feather source contract check: FEATHER_SRC is not set");
        return;
    };
    let feather_src = Path::new(&feather_src);

    assert_source_contains(
        feather_src,
        "src/wizard/offline_tx_signing/PageOTS_ExportUnsignedTx.cpp",
        &[
            r#"setData("xmr-txunsigned""#,
            "unsigned_monero_tx",
            "Transaction (*unsigned_monero_tx)",
        ],
    );
    assert_source_contains(
        feather_src,
        "src/wizard/offline_tx_signing/PageOTS_ImportUnsignedTx.cpp",
        &[
            "Transaction (*unsigned_monero_tx)",
            "loadUnsignedTransactionFromStr(data)",
        ],
    );
    assert_source_contains(
        feather_src,
        "src/wizard/offline_tx_signing/PageOTS_ExportSignedTx.cpp",
        &[
            r#"setData("xmr-txsigned""#,
            "signed_monero_tx",
            "Transaction (*signed_monero_tx)",
        ],
    );
    assert_source_contains(
        feather_src,
        "src/wizard/offline_tx_signing/PageOTS_ImportSignedTx.cpp",
        &[
            "Transaction (*signed_monero_tx)",
            "loadSignedTxFromStr(data)",
        ],
    );
    assert_source_contains(
        feather_src,
        "src/wizard/offline_tx_signing/PageOTS_ExportOutputs.cpp",
        &[r#"setData("xmr-output""#, "exportOutputsToStr"],
    );
    assert_source_contains(
        feather_src,
        "src/wizard/offline_tx_signing/PageOTS_ExportKeyImages.cpp",
        &[r#"setData("xmr-keyimage""#, "exportKeyImagesToStr"],
    );
    assert_source_contains(
        feather_src,
        "src/MainWindow.cpp",
        &["Transaction (*signed_monero_tx)", "loadSignedTxFile(fn)"],
    );
    assert_source_contains(
        feather_src,
        "src/libwalletqt/Wallet.cpp",
        &[
            "loadSignedTx(fileName.toStdString())",
            "submitTransaction(fileName.toStdString())",
        ],
    );
}
