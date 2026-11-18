// Sampling harness around the real wallet2 code, statically linked; built by build_w2harness.sh.
//   w2harness pick <dist.u64> <draws> <seed> <out.u64>
//   w2harness wallet <daemon> <workdir> <seed> <out.jsonl>   (commands on stdin)
// With -DSEEDED_RNG the two entry points of src/crypto/random.c draw from a seeded
// xoshiro256** stream; without it wallet2's own generator runs, as the control.

#include <sys/stat.h>
#include <unistd.h>

#include <cstdint>
#include <cstdio>
#include <cstring>
#include <fstream>
#include <iostream>
#include <sstream>
#include <string>
#include <vector>

#include "common/util.h"
#include "cryptonote_basic/cryptonote_basic_impl.h"
#include "cryptonote_basic/cryptonote_format_utils.h"
#include "misc_log_ex.h"
#include "mnemonics/electrum-words.h"
#include "string_tools.h"
#include "wallet/wallet2.h"

#ifdef SEEDED_RNG
namespace
{
uint64_t rng_state[4] = {0x9E3779B97F4A7C15ull, 0xBF58476D1CE4E5B9ull, 0x94D049BB133111EBull, 0x2545F4914F6CDD1Dull};
inline uint64_t rotl(uint64_t x, int k) { return (x << k) | (x >> (64 - k)); }
uint64_t rng_next()
{
  const uint64_t result = rotl(rng_state[1] * 5, 7) * 9;
  const uint64_t t = rng_state[1] << 17;
  rng_state[2] ^= rng_state[0];
  rng_state[3] ^= rng_state[1];
  rng_state[1] ^= rng_state[2];
  rng_state[0] ^= rng_state[3];
  rng_state[2] ^= t;
  rng_state[3] = rotl(rng_state[3], 45);
  return result;
}
void rng_seed(uint64_t seed)
{
  for (int i = 0; i < 4; ++i)
  {
    seed += 0x9E3779B97F4A7C15ull;
    uint64_t z = seed;
    z = (z ^ (z >> 30)) * 0xBF58476D1CE4E5B9ull;
    z = (z ^ (z >> 27)) * 0x94D049BB133111EBull;
    rng_state[i] = z ^ (z >> 31);
  }
}
}
extern "C" void generate_random_bytes_not_thread_safe(size_t n, void *result)
{
  uint8_t *out = static_cast<uint8_t *>(result);
  while (n > 0)
  {
    const uint64_t v = rng_next();
    const size_t take = n < 8 ? n : 8;
    memcpy(out, &v, take);
    out += take;
    n -= take;
  }
}
extern "C" void add_extra_entropy_not_thread_safe(const void *, size_t) {}
static void seed_rng(uint64_t seed) { rng_seed(seed); }
static const char *rng_kind = "seeded-xoshiro256ss";
#else
static void seed_rng(uint64_t) {}
static const char *rng_kind = "wallet2-native";
#endif

static std::vector<uint64_t> read_offsets(const std::string &path)
{
  std::ifstream f(path, std::ios::binary);
  if (!f)
    throw std::runtime_error("cannot open " + path);
  f.seekg(0, std::ios::end);
  const size_t bytes = f.tellg();
  f.seekg(0);
  std::vector<uint64_t> v(bytes / 8);
  f.read(reinterpret_cast<char *>(v.data()), v.size() * 8);
  return v;
}

static int mode_pick(int argc, char **argv)
{
  if (argc != 6)
  {
    fprintf(stderr, "usage: w2harness pick <offsets.u64> <n> <seed> <out.u64>\n");
    return 2;
  }
  const std::vector<uint64_t> offsets = read_offsets(argv[2]);
  const uint64_t n = std::stoull(argv[3]);
  seed_rng(std::stoull(argv[4]));
  tools::gamma_picker picker(offsets);
  std::vector<uint64_t> picks(n);
  for (uint64_t i = 0; i < n; ++i)
    picks[i] = picker.pick();
  std::ofstream out(argv[5], std::ios::binary);
  out.write(reinterpret_cast<const char *>(picks.data()), picks.size() * 8);
  fprintf(stderr, "rng=%s blocks=%zu num_rct_outs=%llu picks=%llu\n", rng_kind, offsets.size(),
          (unsigned long long)picker.get_num_rct_outs(), (unsigned long long)n);
  return 0;
}

static std::string log_path;
static bool trace_enabled = false;

static std::string drain_trace()
{
  std::string out;
  std::ifstream f(log_path);
  std::string line;
  while (std::getline(f, line))
  {
    size_t p;
    if ((p = line.find("\tpicked ")) != std::string::npos)
    {
      out += 'p';
      out += line.substr(p + 8, line.find(',', p) - (p + 8));
      out += ' ';
    }
    else if ((p = line.find("\tIndex ")) != std::string::npos)
    {
      const size_t a = line.find(": idx ", p);
      const size_t b = line.find(" (real", a);
      const size_t c = line.find("unlocked ", b);
      if (a != std::string::npos && b != std::string::npos && c != std::string::npos)
      {
        out += 'i';
        out += line.substr(a + 6, b - (a + 6));
        out += ':';
        out += line[c + 9];
        out += ' ';
      }
    }
    else if (line.find("\tfake_outputs_count: ") != std::string::npos)
      out += "A ";
    else if (line.find("\tFake output makeup") != std::string::npos)
      out += "F ";
    else if ((p = line.find("\tSelecting real output: ")) != std::string::npos)
    {
      out += 'R';
      out += line.substr(p + 24, line.find(' ', p + 24) - (p + 24));
      out += ' ';
    }
    else if (line.find("has a known ring, reusing") != std::string::npos)
      out += "K ";
    else if (line.find("\tStarting gamma picking") != std::string::npos)
      out += "G ";
    else if (line.find("\tLooking for ") != std::string::npos)
      out += "L ";
    else if (line.find("median offset index is too low") != std::string::npos)
      out += "M ";
    else if (line.find("amount of unique indices is too low") != std::string::npos)
      out += "Q ";
  }
  f.close();
  if (truncate(log_path.c_str(), 0) != 0)
    perror("truncate log");
  if (!out.empty())
    out.pop_back();
  return out;
}

static bool is_transport_error(const std::string &m)
{
  static const char *needles[] = {"no connection to daemon", "Failed to get height", "daemon is busy",
                                  "Could not obtain output distribution", "failed to connect", "Failed to get earliest fork height"};
  for (const char *n : needles)
    if (m.find(n) != std::string::npos)
      return true;
  return false;
}

static std::string json_escape(const std::string &s)
{
  std::string o;
  for (char c : s)
  {
    if (c == '"' || c == '\\')
    {
      o += '\\';
      o += c;
    }
    else if (c == '\n' || c == '\r' || c == '\t')
      o += ' ';
    else
      o += c;
  }
  return o;
}

static void emit_txs(std::ostream &os, tools::wallet2 &w, const tools::wallet2::transfer_container &transfers,
                     const std::vector<tools::wallet2::pending_tx> &ptxs, std::vector<crypto::key_image> &used)
{
  os << "\"txs\":[";
  for (size_t t = 0; t < ptxs.size(); ++t)
  {
    const auto &ptx = ptxs[t];
    if (t)
      os << ',';
    os << "{\"fee\":" << ptx.fee << ",\"n_out\":" << ptx.tx.vout.size()
       << ",\"weight\":" << cryptonote::get_transaction_weight(ptx.tx) << ",\"sel\":[";
    for (size_t i = 0; i < ptx.selected_transfers.size(); ++i)
      os << (i ? "," : "") << ptx.selected_transfers[i];
    os << "],\"inputs\":[";
    bool first = true;
    for (const auto &in : ptx.tx.vin)
    {
      const auto &k = boost::get<cryptonote::txin_to_key>(in);
      used.push_back(k.k_image);
      size_t idx = (size_t)-1;
      for (size_t i : ptx.selected_transfers)
        if (transfers[i].m_key_image == k.k_image)
          idx = i;
      os << (first ? "" : ",") << "{\"t\":" << (long long)idx << ",\"ring\":[";
      first = false;
      const std::vector<uint64_t> abs = cryptonote::relative_output_offsets_to_absolute(k.key_offsets);
      for (size_t i = 0; i < abs.size(); ++i)
        os << (i ? "," : "") << abs[i];
      os << "]}";
    }
    os << "]}";
  }
  os << "]";
}

static int mode_wallet(int argc, char **argv)
{
  if (argc != 6)
  {
    fprintf(stderr, "usage: w2harness wallet <daemon host:port> <workdir> <seed> <out.jsonl>   (commands on stdin)\n");
    return 2;
  }
  const std::string daemon = argv[2];
  const std::string workdir = argv[3];
  const uint64_t seed = std::stoull(argv[4]);
  std::ofstream os(argv[5]);
  mkdir(workdir.c_str(), 0700);
  log_path = workdir + "/wallet2.log";
  unlink(log_path.c_str());
  mlog_configure(log_path, false, 0, 0);
  mlog_set_categories("*:FATAL");

  std::unique_ptr<tools::wallet2> w;
  tools::wallet2::transfer_container transfers;
  const std::vector<uint8_t> extra;
  uint64_t sample_no = 0;

  os << "{\"ev\":\"start\",\"rng\":\"" << rng_kind << "\",\"seed\":" << seed << "}" << std::endl;
  std::string line;
  while (std::getline(std::cin, line))
  {
    std::istringstream in(line);
    std::string cmd;
    in >> cmd;
    if (cmd.empty() || cmd[0] == '#')
      continue;
    if (cmd == "quit")
      break;
    try
    {
      if (cmd == "restore")
      {
        std::string name, word, words;
        in >> name;
        while (in >> word)
          words += (words.empty() ? "" : " ") + word;
        crypto::secret_key recovery;
        std::string language;
        if (!crypto::ElectrumWords::words_to_bytes(words, recovery, language))
          throw std::runtime_error("bad mnemonic");
        const std::string file = workdir + "/" + name;
        unlink(file.c_str());
        unlink((file + ".keys").c_str());
        w.reset(new tools::wallet2(cryptonote::MAINNET, 1, true));
        w->enable_dns(false);
        w->allow_mismatched_daemon_version(true);
        w->set_seed_language(language);
        if (!w->init(daemon, boost::none, "", 0, true, epee::net_utils::ssl_support_t::e_ssl_support_disabled))
          throw std::runtime_error("init failed");
        if (!w->set_ring_database(workdir + "/ringdb"))
          throw std::runtime_error("set_ring_database failed");
        w->generate(file, "", recovery, true, false, false);
        w->set_refresh_from_block_height(0);
        // Seed after key generation so wallet setup does not shift the stream.
        seed_rng(seed);
        for (int attempt = 0;; ++attempt)
        {
          try { w->refresh(true); break; }
          catch (const std::exception &e)
          {
            if (!is_transport_error(e.what()) || attempt >= 200)
              throw;
            usleep(200000);
          }
        }
        w->get_transfers(transfers);
        os << "{\"ev\":\"restored\",\"name\":\"" << name << "\",\"height\":" << w->get_blockchain_current_height()
           << ",\"n_transfers\":" << transfers.size()
           << ",\"address\":\"" << w->get_account().get_public_address_str(cryptonote::MAINNET) << "\"}" << std::endl;
      }
      else if (cmd == "accounts")
      {
        size_t major, minor;
        in >> major >> minor;
        w->set_subaddress_lookahead(major, minor);
        w->expand_subaddresses({(uint32_t)major - 1, (uint32_t)minor - 1});
        w->rescan_blockchain(true, false, false);
        w->refresh(true);
        w->get_transfers(transfers);
        os << "{\"ev\":\"rescanned\",\"n_transfers\":" << transfers.size() << "}" << std::endl;
      }
      else if (cmd == "transfers")
      {
        for (size_t i = 0; i < transfers.size(); ++i)
        {
          const auto &td = transfers[i];
          os << "{\"ev\":\"transfer\",\"i\":" << i << ",\"amount\":" << td.amount() << ",\"height\":" << td.m_block_height
             << ",\"gidx\":" << td.m_global_output_index << ",\"major\":" << td.m_subaddr_index.major
             << ",\"minor\":" << td.m_subaddr_index.minor << ",\"txid\":\"" << epee::string_tools::pod_to_hex(td.m_txid)
             << "\",\"out\":" << td.m_internal_output_index
             << ",\"ki\":\"" << epee::string_tools::pod_to_hex(td.m_key_image) << "\",\"spent\":" << (td.m_spent ? 1 : 0)
             << ",\"frozen\":" << (td.m_frozen ? 1 : 0) << ",\"unlocked\":" << (w->is_transfer_unlocked(td) ? 1 : 0)
             << ",\"coinbase\":" << (td.m_tx.vin.size() == 1 && td.m_tx.vin[0].type() == typeid(cryptonote::txin_gen) ? 1 : 0)
             << "}\n";
        }
        os << "{\"ev\":\"transfers_end\"}" << std::endl;
      }
      else if (cmd == "trace")
      {
        std::string v;
        in >> v;
        trace_enabled = v == "on";
        mlog_set_categories(trace_enabled ? "*:FATAL,wallet.wallet2:DEBUG,verify:ERROR" : "*:FATAL");
        drain_trace();
      }
      else if (cmd == "reseed")
      {
        uint64_t s;
        in >> s;
        seed_rng(s);
      }
      else if (cmd == "transfer" || cmd == "single" || cmd == "sweep_all")
      {
        std::string tag;
        uint64_t n;
        uint32_t priority;
        in >> tag >> n >> priority;
        uint32_t account = 0;
        std::set<uint32_t> subaddrs;
        size_t single_index = 0;
        if (cmd == "single")
          in >> single_index;
        else
        {
          std::string csv;
          in >> account >> csv;
          if (csv != "-")
          {
            std::istringstream cs(csv);
            std::string item;
            while (std::getline(cs, item, ','))
              subaddrs.insert(std::stoul(item));
          }
        }
        std::vector<cryptonote::tx_destination_entry> dsts;
        cryptonote::address_parse_info sweep_to;
        std::string dest;
        while (in >> dest)
        {
          const size_t colon = dest.find(':');
          cryptonote::address_parse_info info;
          if (!cryptonote::get_account_address_from_str(info, cryptonote::MAINNET, dest.substr(0, colon)))
            throw std::runtime_error("bad address");
          sweep_to = info;
          if (colon != std::string::npos)
            dsts.push_back(cryptonote::tx_destination_entry(std::stoull(dest.substr(colon + 1)), info.address, info.is_subaddress));
        }
        for (uint64_t k = 0; k < n; ++k)
        {
          std::vector<crypto::key_image> used;
          std::ostringstream row;
          row << "{\"ev\":\"sample\",\"tag\":\"" << tag << "\",\"k\":" << k << ",\"n\":" << sample_no++ << ",";
          bool ok = true;
          std::string err;
          int transport_retries = 0;
          for (;;)
          {
          try
          {
            const uint32_t adjusted = w->adjust_priority(priority);
            const size_t mixin = w->adjust_mixin(0);
            std::vector<tools::wallet2::pending_tx> ptxs;
            if (cmd == "transfer")
              ptxs = w->create_transactions_2(dsts, mixin, adjusted, extra, account, subaddrs);
            else if (cmd == "single")
              ptxs = w->create_transactions_from(sweep_to.address, sweep_to.is_subaddress, 1, {single_index}, {}, mixin, adjusted, extra);
            else
              ptxs = w->create_transactions_all(0, sweep_to.address, sweep_to.is_subaddress, 1, mixin, adjusted, extra, account, subaddrs);
            row << "\"priority\":" << adjusted << ",";
            emit_txs(row, *w, transfers, ptxs, used);
          }
          catch (const std::exception &e)
          {
            if (is_transport_error(e.what()) && transport_retries < 200)
            {
              ++transport_retries;
              usleep(200000);
              if (trace_enabled)
                drain_trace();
              std::vector<crypto::key_image> all;
              for (const auto &td : transfers)
                all.push_back(td.m_key_image);
              w->unset_ring(all);
              continue;
            }
            ok = false;
            err = e.what();
            row << "\"txs\":[]";
          }
          break;
          }
          row << ",\"ok\":" << (ok ? "true" : "false") << ",\"err\":\"" << json_escape(err) << "\""
              << ",\"transport_retries\":" << transport_retries;
          if (trace_enabled)
            row << ",\"trace\":\"" << drain_trace() << "\"";
          row << "}";
          os << row.str() << "\n";
          // Forget stored rings so the next sample of an input draws a fresh ring.
          if (!ok)
          {
            used.clear();
            for (const auto &td : transfers)
              used.push_back(td.m_key_image);
          }
          if (!used.empty() && !w->unset_ring(used))
            throw std::runtime_error("unset_ring failed");
        }
        os << "{\"ev\":\"cell_end\",\"tag\":\"" << tag << "\"}" << std::endl;
      }
      else
        throw std::runtime_error("unknown command " + cmd);
    }
    catch (const std::exception &e)
    {
      os << "{\"ev\":\"error\",\"cmd\":\"" << json_escape(cmd) << "\",\"err\":\"" << json_escape(e.what()) << "\"}" << std::endl;
    }
  }
  os << "{\"ev\":\"end\"}" << std::endl;
  return 0;
}

int main(int argc, char **argv)
{
  if (argc >= 2 && std::string(argv[1]) == "pick")
    return mode_pick(argc, argv);
  if (argc >= 2 && std::string(argv[1]) == "wallet")
    return mode_wallet(argc, argv);
  fprintf(stderr, "usage: w2harness pick|wallet ...\n");
  return 2;
}
