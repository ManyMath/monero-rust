//! wallet2's built-in block id checkpoints (`checkpoints::init_default_checkpoints`).

use monero_wallet::address::Network;

#[rustfmt::skip]
const MAINNET: &[(u64, &str)] = &[
    (1, "771fbcd656ec1464d3a02ead5e18644030007a0fc664c0a964d30922821a8148"),
    (10, "c0e3b387e47042f72d8ccdca88071ff96bff1ac7cde09ae113dbb7ad3fe92381"),
    (100, "ac3e11ca545e57c49fca2b4e8c48c03c23be047c43e471e1394528b1f9f80b2d"),
    (1000, "5acfc45acffd2b2e7345caf42fa02308c5793f15ec33946e969e829f40b03876"),
    (10000, "c758b7c81f928be3295d45e230646de8b852ec96a821eac3fea4daf3fcac0ca2"),
    (22231, "7cb10e29d67e1c069e6e11b17d30b809724255fee2f6868dc14cfc6ed44dfb25"),
    (29556, "53c484a8ed91e4da621bb2fa88106dbde426fe90d7ef07b9c1e5127fb6f3a7f6"),
    (50000, "0fe8758ab06a8b9cb35b7328fd4f757af530a5d37759f9d3e421023231f7b31c"),
    (80000, "a62dcd7b536f22e003ebae8726e9e7276f63d594e264b6f0cd7aab27b66e75e3"),
    (202612, "bbd604d2ba11ba27935e006ed39c9bfdd99b76bf4a50654bc1e1e61217962698"),
    (202613, "e2aa337e78df1f98f462b3b1e560c6b914dec47b610698b7b7d1e3e86b6197c2"),
    (202614, "c29e3dc37d8da3e72e506e31a213a58771b24450144305bcba9e70fa4d6ea6fb"),
    (205000, "5d3d7a26e6dc7535e34f03def711daa8c263785f73ec1fadef8a45880fde8063"),
    (220000, "9613f455933c00e3e33ac315cc6b455ee8aa0c567163836858c2d9caff111553"),
    (230300, "bae7a80c46859db355556e3a9204a337ae8f24309926a1312323fdecf1920e61"),
    (230700, "93e631240ceac831da1aebfc5dac8f722c430463024763ebafa888796ceaeedf"),
    (231350, "b5add137199b820e1ea26640e5c3e121fd85faa86a1e39cf7e6cc097bdeb1131"),
    (232150, "955de8e6b6508af2c24f7334f97beeea651d78e9ade3ab18fec3763be3201aa8"),
    (249380, "654fb0a81ce3e5caf7e3264a70f447d4bd07586c08fa50f6638cc54da0a52b2d"),
    (460000, "75037a7aed3e765db96c75bcf908f59d690a5f3390baebb9edeafd336a1c4831"),
    (500000, "2428f0dbe49796be05ed81b347f53e1f7f44aed0abf641446ec2b94cae066b02"),
    (600000, "f5828ebf7d7d1cb61762c4dfe3ccf4ecab2e1aad23e8113668d981713b7a54c5"),
    (700000, "12be9b3d210b93f574d2526abb9c1ab2a881b479131fd0d4f7dac93875f503cd"),
    (825000, "56503f9ad766774b575be3aff73245e9d159be88132c93d1754764f28da2ff60"),
    (900000, "d9958d0e7dcf91a5a7b11de225927bf7efc6eb26240315ce12372be902cc1337"),
    (913193, "5292d5d56f6ba4de33a58d9a34d263e2cb3c6fee0aed2286fd4ac7f36d53c85f"),
    (1000000, "a886ef5149902d8342475fee9bb296341b891ac67c4842f47a833f23c00ed721"),
    (1100000, "3fd720c5c8b3072fc1ccda922dec1ef25f9ed88a1e6ad4103d0fe00b180a5903"),
    (1150000, "1dd16f626d18e1e988490dfd06de5920e22629c972c58b4d8daddea0038627b2"),
    (1200000, "fa7d13a90850882060479d100141ff84286599ae39c3277c8ea784393f882d1f"),
    (1300000, "31b34272343a44a9f4ac7de7a8fcf3b7d8a3124d7d6870affd510d2f37e74cd0"),
    (1390000, "a8f5649dd4ded60eedab475f2bec8c934681c07e3cf640e9be0617554f13ff6c"),
    (1450000, "ac94e8860093bc7c83e4e91215cba1d663421ecf4067a0ae609c3a8b52bcfac2"),
    (1530000, "01759bce497ec38e63c78b1038892169203bb78f87e488172f6b854fcd63ba7e"),
    (1579000, "7d0d7a2346373afd41ed1e744a939fc5d474a7dbaa257be5c6fff4009e789241"),
    (1668900, "ac2dcaf3d2f58ffcf8391639f0f1ebafcb8eac43c49479c7c37f611868d07568"),
    (1775600, "1c6e01c661dc22cab939e79ec6a5272190624ce8356d2f7b958e4f9a57fdb05e"),
    (1856000, "9b57f17f29c71a3acd8a7904b93c41fa6eb8d2b7c73936ce4f1702d14880ba29"),
    (1958000, "98a5d6e51afdf3146e0eefb10a66e8648d8d4d5c2742be8835e976ba217c9bb2"),
    (2046000, "5e867f0b8baefed9244a681df97fc885d8ab36c3dfcd24c7a3abf3b8ac8b8314"),
    (2092500, "c4e00820c9c7989b49153d5e90ae095a18a11d990e82fcc3be54e6ed785472b5"),
    (2182500, "0d22b5f81982eff21d094af9e821dc2007e6342069e3b1a37b15d97646353124"),
    (2661600, "41c9060e8426012238e8a26da26fcb90797436896cc70886a894c2c560bcccf2"),
    (2677000, "1b9fee6246eeb176bd17d637bf252e9af54a4218675f01b4449cc0901867f9eb"),
    (2706000, "d8eb144c5e1fe6b329ecc900ec95e7792fccff84175fb23a25ed59d7299a511c"),
    (2720000, "b19fb41dff15bd1016afbee9f8469f05aab715c9e5d1b974466a11fd58ecbb86"),
    (2817000, "39726d19ccaac01d150bec827b877ffae710b516bd633503662036ef4422e577"),
    (2844000, "28fc7b446dfef5b469f5778eb72ddf32a307a5f5a9823d1c394e772349e05d40"),
    (2851000, "5bf0e47fc782263191a33f63a67db6c711781dc2a3c442e17ed901ec401be5c9"),
    (2971000, "3d4cac5ac515eeabd18769ab943af85f36db51d28720def0d0e6effc2c8f5ce3"),
    (2985000, "08f5e6b7301c1b6ed88268a28f8677a06e8ff943b3f9e48d3080f71f9c134bfb"),
    (3088000, "bddf8ca09110d33d6d497f13a113630c2b6af1c84d4f3a6f35cb1446f2604ade"),
    (3102800, "083f4a34f9490403b564286e7f13fd1ed45c52c86fa47195f151594e5bc87504"),
    (3198000, "1d685b39be51e4e84e0af69fa78e023c7cb21de7d33acd012d0371d5f78712d5"),
    (3375700, "96ef57b830ef7a7ccb61ada8595a4670765b6954d8cbf45c6cf583700a676302"),
    (3451000, "0cbc912e06e1adae11f6c9cb675d3159d225b4b04d4a6c61defe50ae1816dd60"),
    (3482000, "629071b10ddad67bdc6156102aba8e008a754c91da252eede852fff9175a9f0a"),
    (3516300, "fa08acbcda99fcc3cd94a749364a29fa6de9501a023cb6673d0c68fdf988b7c3"),
    (3541000, "74c457bed9ceef40f31f43bb8fab804077519d45c910dcad2acf4dd8556195c7"),
    (3576000, "5da4891bfd06be270193bd949f2a623a2b0cb0ebfaad21c70a6cb18e418e5b6a"),
    (3609000, "2c49c7eb40959b4d7a452dcec64e65c59a8b1ebec12ffe0af42bc9468eddae56"),
    (3661900, "ac392757a92123f68d63cd72f0d1410f63df1102a53b5d39fc4d53d0998b20a3"),
    (3774800, "0f4c4c1ca33c35dc56cb764a94e96467df281299e9e5264a5d905309cb806488"),
];

#[rustfmt::skip]
const TESTNET: &[(u64, &str)] = &[
    (0, "48ca7cd3c8de5b6a4d53d2861fbdaedca141553559f9be9520068053cda8430b"),
    (1000000, "46b690b710a07ea051bc4a6b6842ac37be691089c0f7758cfeec4d5fc0b4a258"),
    (1058600, "12904f6b4d9e60fd875674e07147d2c83d6716253f046af7b894c3e81da7e1bd"),
    (1450000, "87562ca6786f41556b8d5b48067303a57dc5ca77155b35199aedaeca1550f5a0"),
];

#[rustfmt::skip]
const STAGENET: &[(u64, &str)] = &[
    (0, "76ee3cc98646292206cd3e86f74d88b4dcc1d937088645e9b0cbca84b7ce74eb"),
    (10000, "1f8b0ce313f8b9ba9a46108bfd285c45ad7c2176871fd41c3a690d4830ce2fd5"),
    (550000, "409f68cddd8e74b37469b41c1e61250d81c5776b42264f416d5d27c4626383ed"),
];

/// The checkpoint wallet2's `fast_refresh` starts from for `stop_height`: none
/// below 1000, else the highest below it (wallet2.cpp:3961); 0 means genesis.
pub(crate) fn fetch_base(network: Network, stop_height: u64) -> (u64, Option<[u8; 32]>) {
    if stop_height < 1000 {
        return (0, None);
    }
    let points = match network {
        Network::Mainnet => MAINNET,
        Network::Testnet => TESTNET,
        Network::Stagenet => STAGENET,
    };
    match points
        .iter()
        .rev()
        .find(|(height, _)| *height < stop_height)
    {
        Some((height, hash)) if *height > 0 => (*height, Some(decode(hash))),
        _ => (0, None),
    }
}

/// The checkpoint id at `height`, if there is one.
pub(crate) fn at(network: Network, height: u64) -> Option<[u8; 32]> {
    let points = match network {
        Network::Mainnet => MAINNET,
        Network::Testnet => TESTNET,
        Network::Stagenet => STAGENET,
    };
    points
        .iter()
        .find(|(point, _)| *point == height)
        .map(|(_, hash)| decode(hash))
}

fn decode(hex: &str) -> [u8; 32] {
    let mut hash = [0u8; 32];
    hex::decode_to_slice(hex, &mut hash).expect("constant checkpoint id");
    hash
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_base_is_wallet2s_nearest_checkpoint_below_the_stop_height() {
        assert_eq!(fetch_base(Network::Mainnet, 999), (0, None));
        assert_eq!(fetch_base(Network::Mainnet, 1000).0, 100);
        assert_eq!(fetch_base(Network::Mainnet, 1001).0, 1000);
        assert_eq!(fetch_base(Network::Mainnet, 3_774_800).0, 3_661_900);
        assert_eq!(fetch_base(Network::Mainnet, 4_000_000).0, 3_774_800);
        assert_eq!(fetch_base(Network::Stagenet, 2_250_000).0, 550_000);
        assert_eq!(fetch_base(Network::Stagenet, 9_000), (0, None));
        assert_eq!(fetch_base(Network::Testnet, 1_000_001).0, 1_000_000);
        assert_eq!(
            hex::encode(fetch_base(Network::Stagenet, 550_001).1.unwrap()),
            "409f68cddd8e74b37469b41c1e61250d81c5776b42264f416d5d27c4626383ed"
        );
        assert_eq!(
            at(Network::Mainnet, 1000),
            fetch_base(Network::Mainnet, 1001).1
        );
        assert_eq!(at(Network::Mainnet, 999), None);
    }
}
