// Generate OutProofV2 vectors and verify external proofs using Monero's
// crypto::generate_tx_proof and crypto::check_tx_proof.
//
//   ./harness gen
//   ./harness check <network> <address> <txid_hex> <R_hex> <message> <OutProofV2...>

#include <cstdio>
#include <cstring>
#include <string>
#include <vector>
#include <iostream>

#include "crypto/crypto.h"
#include "crypto/hash.h"
#include "common/base58.h"
#include "cryptonote_basic/account.h"
#include "cryptonote_basic/cryptonote_basic_impl.h"
#include "cryptonote_basic/subaddress_index.h"
#include "device/device.hpp"
#include "ringct/rctOps.h"

static std::string hex(const void *data, size_t len) {
  static const char *d = "0123456789abcdef";
  const unsigned char *p = (const unsigned char *)data;
  std::string out;
  for (size_t i = 0; i < len; i++) { out += d[p[i] >> 4]; out += d[p[i] & 15]; }
  return out;
}

static bool unhex(const std::string &s, void *out, size_t len) {
  if (s.size() != len * 2) return false;
  unsigned char *p = (unsigned char *)out;
  for (size_t i = 0; i < len; i++) {
    unsigned v = 0;
    if (sscanf(s.c_str() + 2 * i, "%2x", &v) != 1) return false;
    p[i] = (unsigned char)v;
  }
  return true;
}

// Derive deterministic scalars as Hs(label) so the same labels reproduce the keys.
static crypto::secret_key scalar_from(const char *label) {
  crypto::secret_key s;
  crypto::hash_to_scalar(label, strlen(label), s);
  return s;
}

static crypto::public_key mul(const crypto::public_key &P, const crypto::secret_key &s) {
  rct::key out;
  rct::scalarmultKey(out, rct::pk2rct(P), rct::sk2rct(s));
  return rct::rct2pk(out);
}

static crypto::hash prefix_hash_of(const crypto::hash &txid, const std::string &message) {
  std::string prefix_data((const char *)&txid, sizeof(crypto::hash));
  prefix_data += message;
  crypto::hash h;
  crypto::cn_fast_hash(prefix_data.data(), prefix_data.size(), h);
  return h;
}

static std::string json_escape(const std::string &s) {
  std::string out;
  for (char c : s) {
    if (c == '"' || c == '\\') { out += '\\'; out += c; }
    else out += c;
  }
  return out;
}

struct Case {
  const char *name;
  bool is_subaddress;
  const char *message;
};

static int gen() {
  hw::device &hwdev = hw::get_device("default");

  const crypto::secret_key view_sec  = scalar_from("monero-rust out-proof vector: view");
  const crypto::secret_key spend_sec = scalar_from("monero-rust out-proof vector: spend");
  const crypto::secret_key tx_sec    = scalar_from("monero-rust out-proof vector: tx key");

  cryptonote::account_keys keys;
  keys.m_view_secret_key = view_sec;
  keys.m_spend_secret_key = spend_sec;
  crypto::secret_key_to_public_key(view_sec, keys.m_account_address.m_view_public_key);
  crypto::secret_key_to_public_key(spend_sec, keys.m_account_address.m_spend_public_key);
  keys.m_device = &hwdev;

  const cryptonote::subaddress_index sub_index = {0, 1};
  const cryptonote::account_public_address sub_addr = hwdev.get_subaddress(keys, sub_index);

  crypto::hash txid;
  if (!unhex("46d9f3eaf8d25b6a5d0847ad0beaece8b153d1b8c25ce317934ec17223025806", &txid, 32)) {
    std::cerr << "bad txid" << std::endl;
    return 1;
  }

  const Case cases[] = {
    {"standard_no_message",   false, ""},
    {"standard_with_message", false, "monero-rust cross-implementation vector"},
    {"subaddress_no_message", true,  ""},
    {"subaddress_with_message", true, "monero-rust cross-implementation vector"},
  };

  printf("{\n");
  printf("  \"txid\": \"%s\",\n", hex(&txid, 32).c_str());
  printf("  \"network\": \"mainnet\",\n");
  printf("  \"tx_secret_key\": \"%s\",\n", hex(&tx_sec, 32).c_str());
  printf("  \"recipient\": {\n");
  printf("    \"view_secret_key\": \"%s\",\n", hex(&view_sec, 32).c_str());
  printf("    \"spend_secret_key\": \"%s\",\n", hex(&spend_sec, 32).c_str());
  printf("    \"subaddress_index\": [%u, %u]\n", sub_index.major, sub_index.minor);
  printf("  },\n");
  printf("  \"cases\": [\n");

  bool first = true;
  for (const Case &c : cases) {
    const cryptonote::account_public_address &addr =
      c.is_subaddress ? sub_addr : keys.m_account_address;
    const std::string message = c.message;
    const crypto::hash prefix_hash = prefix_hash_of(txid, message);

    // Follow the outbound proof branch of wallet2::get_tx_proof.
    const crypto::public_key D = mul(addr.m_view_public_key, tx_sec);
    crypto::public_key R;
    crypto::signature sig;
    if (c.is_subaddress) {
      R = mul(addr.m_spend_public_key, tx_sec);
      crypto::generate_tx_proof(prefix_hash, R, addr.m_view_public_key,
                                addr.m_spend_public_key, D, tx_sec, sig);
    } else {
      crypto::secret_key_to_public_key(tx_sec, R);
      crypto::generate_tx_proof(prefix_hash, R, addr.m_view_public_key,
                                boost::none, D, tx_sec, sig);
    }

    const bool self_check = crypto::check_tx_proof(
      prefix_hash, R, addr.m_view_public_key,
      c.is_subaddress ? boost::optional<crypto::public_key>(addr.m_spend_public_key)
                      : boost::none,
      D, sig, 2);
    if (!self_check) {
      std::cerr << "monero rejected its own proof for case " << c.name << std::endl;
      return 1;
    }

    const std::string sig_str =
      std::string("OutProofV2") +
      tools::base58::encode(std::string((const char *)&D, sizeof(crypto::public_key))) +
      tools::base58::encode(std::string((const char *)&sig, sizeof(crypto::signature)));

    if (!first) printf(",\n");
    first = false;
    printf("    {\n");
    printf("      \"name\": \"%s\",\n", c.name);
    printf("      \"is_subaddress\": %s,\n", c.is_subaddress ? "true" : "false");
    printf("      \"address\": \"%s\",\n",
           cryptonote::get_account_address_as_str(cryptonote::MAINNET, c.is_subaddress, addr).c_str());
    printf("      \"message\": \"%s\",\n", json_escape(message).c_str());
    printf("      \"tx_public_key\": \"%s\",\n", hex(&R, 32).c_str());
    printf("      \"shared_secret\": \"%s\",\n", hex(&D, 32).c_str());
    printf("      \"signature\": \"%s\"\n", sig_str.c_str());
    printf("    }");
  }
  printf("\n  ]\n}\n");
  return 0;
}

static int check(int argc, char **argv) {
  if (argc != 8) {
    std::cerr << "usage: harness check <network> <address> <txid_hex> <R_hex> <message> <proof>"
              << std::endl;
    return 2;
  }
  const std::string network = argv[2], address_str = argv[3], txid_hex = argv[4],
                    r_hex = argv[5], message = argv[6], proof = argv[7];

  cryptonote::network_type nettype = cryptonote::MAINNET;
  if (network == "stagenet") nettype = cryptonote::STAGENET;
  else if (network == "testnet") nettype = cryptonote::TESTNET;
  else if (network != "mainnet") { std::cerr << "bad network" << std::endl; return 2; }

  cryptonote::address_parse_info info;
  if (!cryptonote::get_account_address_from_str(info, nettype, address_str)) {
    std::cerr << "bad address" << std::endl;
    return 2;
  }

  crypto::hash txid;
  crypto::public_key R;
  if (!unhex(txid_hex, &txid, 32) || !unhex(r_hex, &R, 32)) {
    std::cerr << "bad hex" << std::endl;
    return 2;
  }

  if (proof.rfind("OutProofV2", 0) != 0) { std::cerr << "not OutProofV2" << std::endl; return 2; }
  std::string payload;
  if (!tools::base58::decode(proof.substr(strlen("OutProofV2")), payload) || payload.size() != 96) {
    std::cerr << "bad base58 payload (" << payload.size() << " bytes)" << std::endl;
    return 2;
  }
  crypto::public_key D;
  crypto::signature sig;
  memcpy(&D, payload.data(), 32);
  memcpy(&sig, payload.data() + 32, 64);

  const crypto::hash prefix_hash = prefix_hash_of(txid, message);
  const bool ok = crypto::check_tx_proof(
    prefix_hash, R, info.address.m_view_public_key,
    info.is_subaddress ? boost::optional<crypto::public_key>(info.address.m_spend_public_key)
                       : boost::none,
    D, sig, 2);
  printf("%s\n", ok ? "VALID" : "INVALID");
  return ok ? 0 : 1;
}

int main(int argc, char **argv) {
  if (argc >= 2 && strcmp(argv[1], "gen") == 0) return gen();
  if (argc >= 2 && strcmp(argv[1], "check") == 0) return check(argc, argv);
  std::cerr << "usage: harness gen | harness check ..." << std::endl;
  return 2;
}
