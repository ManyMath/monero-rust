use zeroize::Zeroizing;

use rand_core::OsRng;

use curve25519_dalek::{constants::ED25519_BASEPOINT_TABLE, scalar::Scalar};

use crate::{
  hash, hash_to_scalar,
  wallet::{
    seed::{Seed, Language, classic::trim_by_lang},
    address::{Network, AddressSpec, MoneroAddress},
    ViewPair,
  },
};

#[test]
fn test_classic_seed() {
  struct Vector {
    language: Language,
    seed: String,
    spend: String,
    view: String,
  }

  let vectors = [
    Vector {
      language: Language::Chinese,
      seed: "摇 曲 艺 武 滴 然 效 似 赏 式 祥 歌 买 疑 小 碧 堆 博 键 房 鲜 悲 付 喷 武".into(),
      spend: "a5e4fff1706ef9212993a69f246f5c95ad6d84371692d63e9bb0ea112a58340d".into(),
      view: "1176c43ce541477ea2f3ef0b49b25112b084e26b8a843e1304ac4677b74cdf02".into(),
    },
    Vector {
      language: Language::English,
      seed: "washing thirsty occur lectures tuesday fainted toxic adapt \
               abnormal memoir nylon mostly building shrugged online ember northern \
               ruby woes dauntless boil family illness inroads northern"
        .into(),
      spend: "c0af65c0dd837e666b9d0dfed62745f4df35aed7ea619b2798a709f0fe545403".into(),
      view: "513ba91c538a5a9069e0094de90e927c0cd147fa10428ce3ac1afd49f63e3b01".into(),
    },
    Vector {
      language: Language::Dutch,
      seed: "setwinst riphagen vimmetje extase blief tuitelig fuiven meifeest \
               ponywagen zesmaal ripdeal matverf codetaal leut ivoor rotten \
               wisgerhof winzucht typograaf atrium rein zilt traktaat verzaagd setwinst"
        .into(),
      spend: "e2d2873085c447c2bc7664222ac8f7d240df3aeac137f5ff2022eaa629e5b10a".into(),
      view: "eac30b69477e3f68093d131c7fd961564458401b07f8c87ff8f6030c1a0c7301".into(),
    },
    Vector {
      language: Language::French,
      seed: "poids vaseux tarte bazar poivre effet entier nuance \
               sensuel ennui pacte osselet poudre battre alibi mouton \
               stade paquet pliage gibier type question position projet pliage"
        .into(),
      spend: "2dd39ff1a4628a94b5c2ec3e42fb3dfe15c2b2f010154dc3b3de6791e805b904".into(),
      view: "6725b32230400a1032f31d622b44c3a227f88258939b14a7c72e00939e7bdf0e".into(),
    },
    Vector {
      language: Language::Spanish,
      seed: "minero ocupar mirar evadir octubre cal logro miope \
               opaco disco ancla litio clase cuello nasal clase \
               fiar avance deseo mente grumo negro cordón croqueta clase"
        .into(),
      spend: "ae2c9bebdddac067d73ec0180147fc92bdf9ac7337f1bcafbbe57dd13558eb02".into(),
      view: "18deafb34d55b7a43cae2c1c1c206a3c80c12cc9d1f84640b484b95b7fec3e05".into(),
    },
    Vector {
      language: Language::German,
      seed: "Kaliber Gabelung Tapir Liveband Favorit Specht Enklave Nabel \
               Jupiter Foliant Chronik nisten löten Vase Aussage Rekord \
               Yeti Gesetz Eleganz Alraune Künstler Almweide Jahr Kastanie Almweide"
        .into(),
      spend: "79801b7a1b9796856e2397d862a113862e1fdc289a205e79d8d70995b276db06".into(),
      view: "99f0ec556643bd9c038a4ed86edcb9c6c16032c4622ed2e000299d527a792701".into(),
    },
    Vector {
      language: Language::Italian,
      seed: "cavo pancetta auto fulmine alleanza filmato diavolo prato \
               forzare meritare litigare lezione segreto evasione votare buio \
               licenza cliente dorso natale crescere vento tutelare vetta evasione"
        .into(),
      spend: "5e7fd774eb00fa5877e2a8b4dc9c7ffe111008a3891220b56a6e49ac816d650a".into(),
      view: "698a1dce6018aef5516e82ca0cb3e3ec7778d17dfb41a137567bfa2e55e63a03".into(),
    },
    Vector {
      language: Language::Portuguese,
      seed: "agito eventualidade onus itrio holograma sodomizar objetos dobro \
               iugoslavo bcrepuscular odalisca abjeto iuane darwinista eczema acetona \
               cibernetico hoquei gleba driver buffer azoto megera nogueira agito"
        .into(),
      spend: "13b3115f37e35c6aa1db97428b897e584698670c1b27854568d678e729200c0f".into(),
      view: "ad1b4fd35270f5f36c4da7166672b347e75c3f4d41346ec2a06d1d0193632801".into(),
    },
    Vector {
      language: Language::Japanese,
      seed: "ぜんぶ どうぐ おたがい せんきょ おうじ そんちょう じゅしん いろえんぴつ \
               かほう つかれる えらぶ にちじょう くのう にちようび ぬまえび さんきゃく \
               おおや ちぬき うすめる いがく せつでん さうな すいえい せつだん おおや"
        .into(),
      spend: "c56e895cdb13007eda8399222974cdbab493640663804b93cbef3d8c3df80b0b".into(),
      view: "6c3634a313ec2ee979d565c33888fd7c3502d696ce0134a8bc1a2698c7f2c508".into(),
    },
    Vector {
      language: Language::Russian,
      seed: "шатер икра нация ехать получать инерция доза реальный \
               рыжий таможня лопата душа веселый клетка атлас лекция \
               обгонять паек наивный лыжный дурак стать ежик задача паек"
        .into(),
      spend: "7cb5492df5eb2db4c84af20766391cd3e3662ab1a241c70fc881f3d02c381f05".into(),
      view: "fcd53e41ec0df995ab43927f7c44bc3359c93523d5009fb3f5ba87431d545a03".into(),
    },
    Vector {
      language: Language::Esperanto,
      seed: "ukazo klini peco etikedo fabriko imitado onklino urino \
               pudro incidento kumuluso ikono smirgi hirundo uretro krii \
               sparkado super speciala pupo alpinisto cvana vokegi zombio fabriko"
        .into(),
      spend: "82ebf0336d3b152701964ed41df6b6e9a035e57fc98b84039ed0bd4611c58904".into(),
      view: "cd4d120e1ea34360af528f6a3e6156063312d9cefc9aa6b5218d366c0ed6a201".into(),
    },
    Vector {
      language: Language::Lojban,
      seed: "jetnu vensa julne xrotu xamsi julne cutci dakli \
               mlatu xedja muvgau palpi xindo sfubu ciste cinri \
               blabi darno dembi janli blabi fenki bukpu burcu blabi"
        .into(),
      spend: "e4f8c6819ab6cf792cebb858caabac9307fd646901d72123e0367ebc0a79c200".into(),
      view: "c806ce62bafaa7b2d597f1a1e2dbe4a2f96bfd804bf6f8420fc7f4a6bd700c00".into(),
    },
    Vector {
      language: Language::EnglishOld,
      seed: "glorious especially puff son moment add youth nowhere \
               throw glide grip wrong rhythm consume very swear \
               bitter heavy eventually begin reason flirt type unable"
        .into(),
      spend: "647f4765b66b636ff07170ab6280a9a6804dfbaf19db2ad37d23be024a18730b".into(),
      view: "045da65316a906a8c30046053119c18020b07a7a3a6ef5c01ab2a8755416bd02".into(),
    },
  ];

  for vector in vectors {
    let trim_seed = |seed: &str| {
      seed
        .split_whitespace()
        .map(|word| trim_by_lang(word, vector.language))
        .collect::<Vec<_>>()
        .join(" ")
    };

    // Test against Monero
    {
      let seed = Seed::from_string(Zeroizing::new(vector.seed.clone())).unwrap();
      assert_eq!(seed, Seed::from_string(Zeroizing::new(trim_seed(&vector.seed))).unwrap());

      let spend: [u8; 32] = hex::decode(vector.spend).unwrap().try_into().unwrap();
      // For classical seeds, Monero directly uses the entropy as a spend key
      assert_eq!(
        Scalar::from_canonical_bytes(*seed.entropy()),
        Scalar::from_canonical_bytes(spend)
      );

      let view: [u8; 32] = hex::decode(vector.view).unwrap().try_into().unwrap();
      // Monero then derives the view key as H(spend)
      assert_eq!(
        Scalar::from_bytes_mod_order(hash(&spend)),
        Scalar::from_canonical_bytes(view).unwrap()
      );

      assert_eq!(Seed::from_entropy(vector.language, Zeroizing::new(spend)).unwrap(), seed);
    }

    // Test against ourself
    {
      let seed = Seed::new(&mut OsRng, vector.language);
      assert_eq!(seed, Seed::from_string(Zeroizing::new(trim_seed(&seed.to_string()))).unwrap());
      assert_eq!(seed, Seed::from_entropy(vector.language, seed.entropy()).unwrap());
      assert_eq!(seed, Seed::from_string(seed.to_string()).unwrap());
    }
  }
}

#[test]
fn test_polyseed() {
  use polyseed::{Polyseed, Language as PolyseedLanguage};

  let seed_phrase = "capital chief route liar question fix clutch water outside pave hamster occur always learn license knife";

  let polyseed = Polyseed::from_string(
    PolyseedLanguage::English,
    Zeroizing::new(seed_phrase.to_string())
  ).unwrap();

  // Derive the spend key from polyseed
  let key = polyseed.key();
  let spend_scalar = Scalar::from_bytes_mod_order(*key);
  assert_ne!(spend_scalar, Scalar::zero());
  let spend_key = &spend_scalar * &ED25519_BASEPOINT_TABLE;

  // Derive view key as H(spend_scalar)
  let view_scalar = hash_to_scalar(&spend_scalar.to_bytes());
  assert_ne!(view_scalar, Scalar::zero());
  let view_key = &view_scalar * &ED25519_BASEPOINT_TABLE;

  // Create ViewPair and generate address
  let view_pair = ViewPair::new(spend_key, Zeroizing::new(view_scalar));
  let address = view_pair.address(Network::Mainnet, AddressSpec::Standard);

  // Expected values from test vector
  let expected_address = "465cUW8wTMSCV8oVVh7CuWWHs7yeB1oxhNPrsEM5FKSqadTXmobLqsNEtRnyGsbN1rbDuBtWdtxtXhTJda1Lm9vcH2ZdrD1";
  let expected_spend_key = "c584b326f1a8472e210d80e4fc87271ffa371f94b95a0794eef80e851fb4e303";
  let expected_view_key = "3b8ffd9a88e9cdbbd311629c38d696df07551bcea08e0df1942507db8f832007";
  let expected_public_spend_key = "759ca40019178944aa2fe8062dfe61af1e3678be2ceed67fe83c34edde8492c9";
  let expected_public_view_key = "0d57d0165de6015305e5c1e2c54f75cc9a385348929980f1db140ac459e9958e";

  // Verify address
  assert_eq!(
    address.to_string(),
    expected_address,
    "Address mismatch"
  );

  // Verify secret spend key
  assert_eq!(
    hex::encode(spend_scalar.to_bytes()),
    expected_spend_key,
    "Secret spend key mismatch"
  );

  // Verify secret view key
  assert_eq!(
    hex::encode(view_scalar.to_bytes()),
    expected_view_key,
    "Secret view key mismatch"
  );

  // Verify public spend key
  assert_eq!(
    hex::encode(spend_key.compress().to_bytes()),
    expected_public_spend_key,
    "Public spend key mismatch"
  );

  // Verify public view key
  assert_eq!(
    hex::encode(view_key.compress().to_bytes()),
    expected_public_view_key,
    "Public view key mismatch"
  );

  // Additional validation: parse the expected address and verify keys match
  let parsed_address = MoneroAddress::from_str(Network::Mainnet, expected_address).unwrap();
  assert_eq!(
    parsed_address.spend,
    spend_key,
    "Parsed address spend key doesn't match derived spend key"
  );
  assert_eq!(
    parsed_address.view,
    view_key,
    "Parsed address view key doesn't match derived view key"
  );
}
