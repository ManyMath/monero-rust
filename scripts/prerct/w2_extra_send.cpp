// Send one payment with wallet2's own construction code, in forms that
// monero-wallet-rpc and monero-wallet-cli no longer offer, for scripts/prerct_chain.py.
//
// usage: w2_extra_send <daemon host:port> <ringdb dir> <spend key hex>
//                      <dest address> <amount atomic> <payment id hex64 | ->
//                      <ring size, 0 for the minimum>
//                      <pre-RingCT input amounts allowed, comma separated | ->
//                      <allow RingCT inputs 0|1>
//                      <rct mode: auto|borromean|bp1|bp2> <fee atomic> <min inputs>
//
// The spend key is passed on the command line: throwaway regtest wallets only.

#include <algorithm>
#include <iostream>
#include <set>
#include <sstream>
#include <string>
#include <unordered_set>
#include <vector>

#include "cryptonote_basic/cryptonote_basic_impl.h"
#include "cryptonote_basic/cryptonote_format_utils.h"
#include "string_tools.h"
#include "wallet/wallet2.h"

int main(int argc, char **argv)
{
  if (argc != 13)
  {
    std::cerr << "bad arguments, see the usage comment in w2_extra_send.cpp" << std::endl;
    return 2;
  }
  const std::string daemon = argv[1];
  const std::string ringdb = argv[2];
  const std::string spend_hex = argv[3];
  const std::string dest = argv[4];
  const uint64_t amount = std::stoull(argv[5]);
  const std::string pid_hex = argv[6];
  const size_t ring_size = std::stoul(argv[7]);
  std::set<uint64_t> allowed;
  const bool allow_any = std::string(argv[8]) == "-";
  if (!allow_any)
  {
    std::stringstream list(argv[8]);
    std::string item;
    while (std::getline(list, item, ','))
      allowed.insert(std::stoull(item));
  }
  const bool allow_rct = std::string(argv[9]) == "1";
  const std::string rct_mode = argv[10];
  const uint64_t fixed_fee = std::stoull(argv[11]);
  const size_t min_inputs = std::stoul(argv[12]);

  try
  {
    mlog_configure("", false);
    mlog_set_log_level(0);

    crypto::secret_key spend;
    if (!epee::string_tools::hex_to_pod(spend_hex, spend))
      throw std::runtime_error("bad spend key");

    tools::wallet2 w(cryptonote::MAINNET, 1, true);
    if (!w.set_ring_database(ringdb))
      throw std::runtime_error("ring database");
    if (!w.init(daemon, boost::none, "", 0, true, epee::net_utils::ssl_support_t::e_ssl_support_disabled))
      throw std::runtime_error("init");
    w.generate("", "", spend, true, false, false);
    w.allow_mismatched_daemon_version(true);
    w.set_refresh_from_block_height(0);
    w.refresh(true);

    tools::wallet2::transfer_container transfers;
    w.get_transfers(transfers);
    for (size_t i = 0; i < transfers.size(); ++i)
    {
      const auto &td = transfers[i];
      if (td.m_spent)
        continue;
      if (td.is_rct() ? !allow_rct : (!allow_any && !allowed.count(td.amount())))
        w.freeze(i);
    }

    cryptonote::address_parse_info info;
    if (!cryptonote::get_account_address_from_str(info, cryptonote::MAINNET, dest))
      throw std::runtime_error("bad destination");

    std::vector<uint8_t> extra;
    if (pid_hex != "-")
    {
      crypto::hash pid;
      if (!epee::string_tools::hex_to_pod(pid_hex, pid))
        throw std::runtime_error("bad payment id");
      std::string nonce;
      cryptonote::set_payment_id_to_tx_extra_nonce(nonce, pid);
      if (!cryptonote::add_extra_nonce_to_tx_extra(extra, nonce))
        throw std::runtime_error("extra nonce");
    }

    if (info.has_payment_id)
    {
      std::string nonce;
      cryptonote::set_encrypted_payment_id_to_tx_extra_nonce(nonce, info.payment_id);
      if (!cryptonote::add_extra_nonce_to_tx_extra(extra, nonce))
        throw std::runtime_error("extra nonce");
    }

    std::vector<cryptonote::tx_destination_entry> dsts;
    dsts.emplace_back(amount, info.address, info.is_subaddress);

    const size_t mixin = w.adjust_mixin(ring_size ? ring_size - 1 : 0);
    std::vector<tools::wallet2::pending_tx> ptxs;
    if (rct_mode == "auto")
    {
      ptxs = w.create_transactions_2(dsts, mixin, 1, extra, 0, {});
    }
    else
    {
      rct::RCTConfig config;
      if (rct_mode == "borromean")
        config = { rct::RangeProofBorromean, 0 };
      else if (rct_mode == "bp1")
        config = { rct::RangeProofPaddedBulletproof, 1 };
      else if (rct_mode == "bp2")
        config = { rct::RangeProofPaddedBulletproof, 2 };
      else
        throw std::runtime_error("bad rct mode");

      w.get_transfers(transfers);
      std::vector<size_t> candidates;
      for (size_t i = 0; i < transfers.size(); ++i)
        if (!transfers[i].m_spent && !transfers[i].m_frozen && w.is_transfer_unlocked(transfers[i]))
          candidates.push_back(i);
      std::sort(candidates.begin(), candidates.end(), [&](size_t a, size_t b) { return transfers[a].amount() > transfers[b].amount(); });
      std::vector<size_t> selected;
      uint64_t found = 0;
      for (size_t i: candidates)
      {
        if (found >= amount + fixed_fee && selected.size() >= min_inputs)
          break;
        selected.push_back(i);
        found += transfers[i].amount();
      }
      if (found < amount + fixed_fee || selected.size() < min_inputs)
        throw std::runtime_error("not enough unlocked money in allowed outputs");

      std::vector<std::vector<tools::wallet2::get_outs_entry>> outs;
      std::unordered_set<crypto::public_key> valid_public_keys_cache;
      ptxs.resize(1);
      cryptonote::transaction tx;
      w.transfer_selected_rct(dsts, selected, mixin, outs, valid_public_keys_cache, fixed_fee, extra, tx, ptxs[0], config, false);
    }
    std::cout << "{\"txids\":[";
    for (size_t i = 0; i < ptxs.size(); ++i)
    {
      w.commit_tx(ptxs[i]);
      std::cout << (i ? "," : "") << "\"" << epee::string_tools::pod_to_hex(cryptonote::get_transaction_hash(ptxs[i].tx)) << "\"";
    }
    std::cout << "],\"version\":" << (ptxs.empty() ? 0 : ptxs[0].tx.version) << ",\"mixin\":" << mixin << ",\"mode\":\"" << rct_mode << "\"}" << std::endl;
    w.deinit();
    return 0;
  }
  catch (const std::exception &e)
  {
    std::cout << "{\"error\":\"" << e.what() << "\"}" << std::endl;
    return 1;
  }
}
