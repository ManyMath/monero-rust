"""Statistics shared by the stat-parity analysis scripts."""
import math

import numpy as np
from scipy import stats

ALPHA = 0.05


def dkw_eps(n, alpha=ALPHA):
    return math.sqrt(math.log(2 / alpha) / (2 * n))


def ks_crit(n, m, alpha):
    return math.sqrt(-0.5 * math.log(alpha / 2)) * math.sqrt((n + m) / (n * m))


def ks(name, a, b, note=""):
    res = stats.ks_2samp(a, b, method="asymp")
    n, m = len(a), len(b)
    d = float(res.statistic)
    eps = dkw_eps(n, ALPHA / 2) + dkw_eps(m, ALPHA / 2)
    return {
        "name": name, "test": "two-sample Kolmogorov-Smirnov", "n1": n, "n2": m,
        "statistic": d, "p": float(res.pvalue),
        "effect": "sup |F1 - F2|", "effect_value": d, "ci": [max(0.0, d - eps), d + eps],
        "mde": ks_crit(n, m, 0.001) + ks_crit(n, m, 0.4),
        "mde_unit": "CDF gap", "note": note,
    }


def cramers_v(table):
    table = np.asarray(table, dtype=float)
    chi2 = stats.chi2_contingency(table, correction=False)[0]
    n = table.sum()
    k = min(table.shape) - 1
    return math.sqrt(chi2 / (n * k)) if n > 0 and k > 0 else 0.0


def permutation_chi2(table, resamples, rng):
    table = np.asarray(table, dtype=np.int64)
    observed = _chi2_stat(table)
    rows = np.repeat(np.arange(table.shape[0]), table.sum(axis=1))
    cols = np.concatenate([np.repeat(np.arange(table.shape[1]), table[r]) for r in range(table.shape[0])])
    hits = 0
    for _ in range(resamples):
        perm = rng.permutation(cols)
        t = np.zeros_like(table)
        np.add.at(t, (rows, perm), 1)
        if _chi2_stat(t) >= observed - 1e-12:
            hits += 1
    return (hits + 1) / (resamples + 1), observed


def _chi2_stat(table):
    table = np.asarray(table, dtype=float)
    expected = np.outer(table.sum(axis=1), table.sum(axis=0)) / table.sum()
    mask = expected > 0
    return float((((table - expected) ** 2)[mask] / expected[mask]).sum())


def contingency(name, counts1, counts2, labels, note="", seed=20261118):
    c1 = np.asarray(counts1, dtype=np.int64)
    c2 = np.asarray(counts2, dtype=np.int64)
    keep = (c1 + c2) > 0
    c1k, c2k = c1[keep], c2[keep]
    n1, n2 = int(c1.sum()), int(c2.sum())
    out = {"name": name, "n1": n1, "n2": n2, "note": note, "categories": len(labels),
           "effect": "largest share difference over categories (share1 - share2)", "mde_unit": "share"}
    rng = np.random.default_rng(seed)
    if n1 == 0 or n2 == 0:
        out.update(test="none (empty sample)", statistic=float("nan"), p=float("nan"), ci=[float("nan")] * 2,
                   mde=float("nan"), effect_value=float("nan"))
        return out
    p1, p2 = c1 / n1, c2 / n2
    diff = p1 - p2
    worst = int(np.argmax(np.abs(diff)))
    z = stats.norm.ppf(1 - ALPHA / (2 * max(int(keep.sum()), 1)))
    half = z * math.sqrt(p1[worst] * (1 - p1[worst]) / n1 + p2[worst] * (1 - p2[worst]) / n2)
    out["effect_value"] = float(diff[worst])
    out["ci"] = [float(diff[worst] - half), float(diff[worst] + half)]
    out["note"] = (f"largest difference in category '{labels[worst]}' ({p1[worst]:.5g} vs {p2[worst]:.5g}); "
                   f"total variation distance {0.5 * float(np.abs(diff).sum()):.4g}. {note}").strip()
    if keep.sum() < 2:
        out.update(test="identical single category", statistic=0.0, p=1.0)
    else:
        table = np.vstack([c1k, c2k])
        expected = np.outer(table.sum(axis=1), table.sum(axis=0)) / table.sum()
        if expected.min() >= 5:
            chi2, p, dof, _ = stats.chi2_contingency(table, correction=False)
            out.update(test=f"Pearson chi-square (dof {dof})", statistic=float(chi2), p=float(p))
        elif table.shape[1] == 2:
            _, p = stats.fisher_exact(table)
            out.update(test="Fisher exact (2x2)", statistic=float("nan"), p=float(p))
        else:
            p, chi2 = permutation_chi2(table, 20000, rng)
            out.update(test="chi-square, Monte Carlo permutation (20000 resamples)", statistic=chi2, p=float(p))
    out["mde"] = (3.29 + 0.84) * math.sqrt(0.25 * (1 / n1 + 1 / n2))
    out["table"] = {"labels": list(labels), "counts1": c1.tolist(), "counts2": c2.tolist()}
    return out


def two_proportions(name, x1, n1, x2, n2, note=""):
    if n1 == 0 or n2 == 0:
        return {"name": name, "test": "none (empty sample)", "n1": n1, "n2": n2, "statistic": float("nan"),
                "p": float("nan"), "effect": "p1 - p2", "effect_value": float("nan"), "ci": [float("nan")] * 2,
                "mde": float("nan"), "mde_unit": "proportion", "note": note}
    p1, p2 = x1 / n1, x2 / n2
    if max(n1, n2) <= 200000:
        _, p = stats.fisher_exact([[x1, n1 - x1], [x2, n2 - x2]])
        test = "Fisher exact"
    else:
        pooled = (x1 + x2) / (n1 + n2)
        se = math.sqrt(pooled * (1 - pooled) * (1 / n1 + 1 / n2))
        z = (p1 - p2) / se if se > 0 else 0.0
        p = 2 * stats.norm.sf(abs(z))
        test = "two-proportion z"

    def wilson(x, n):
        z = 1.959964
        centre = (x + z * z / 2) / (n + z * z)
        half = z * math.sqrt(x * (n - x) / n + z * z / 4) / (n + z * z)
        return centre - half, centre + half

    l1, u1 = wilson(x1, n1)
    l2, u2 = wilson(x2, n2)
    diff = p1 - p2
    lo = diff - math.sqrt((p1 - l1) ** 2 + (u2 - p2) ** 2)
    hi = diff + math.sqrt((u1 - p1) ** 2 + (p2 - l2) ** 2)
    pbar = max((x1 + x2) / (n1 + n2), 1 / (n1 + n2))
    return {"name": name, "test": test, "n1": n1, "n2": n2, "statistic": float("nan"), "p": float(p),
            "effect": "p1 - p2", "effect_value": diff, "ci": [lo, hi],
            "mde": (3.29 + 0.84) * math.sqrt(pbar * (1 - pbar) * (1 / n1 + 1 / n2)),
            "mde_unit": "proportion", "note": f"p1={p1:.6g} ({x1}/{n1}), p2={p2:.6g} ({x2}/{n2}). {note}".strip()}


def mean_difference(name, a, b, note="", seed=20261118):
    a = np.asarray(a, dtype=float)
    b = np.asarray(b, dtype=float)
    out = {"name": name, "test": "Mann-Whitney U", "n1": len(a), "n2": len(b), "effect": "mean1 - mean2",
           "mde_unit": "same unit", "note": note}
    if len(a) == 0 or len(b) == 0:
        out.update(test="none (empty sample)", statistic=float("nan"), p=float("nan"),
                   effect_value=float("nan"), ci=[float("nan")] * 2, mde=float("nan"))
        return out
    if np.all(a == a[0]) and np.all(b == b[0]) and a[0] == b[0]:
        u, p = float("nan"), 1.0
    else:
        u, p = stats.mannwhitneyu(a, b, alternative="two-sided")
    rng = np.random.default_rng(seed)
    boot = [rng.choice(a, len(a)).mean() - rng.choice(b, len(b)).mean() for _ in range(2000)]
    se = math.sqrt(a.var(ddof=1) / len(a) + b.var(ddof=1) / len(b)) if len(a) > 1 and len(b) > 1 else 0.0
    out.update(statistic=float(u), p=float(p), effect_value=float(a.mean() - b.mean()),
               ci=[float(np.percentile(boot, 2.5)), float(np.percentile(boot, 97.5))],
               mde=(3.29 + 0.84) * se)
    out["note"] = f"mean1={a.mean():.6g}, mean2={b.mean():.6g}. {note}".strip()
    return out


def goodness_of_fit(name, counts, note=""):
    counts = np.asarray(counts, dtype=float)
    n = counts.sum()
    chi2, p = stats.chisquare(counts)
    share = counts / n
    worst = int(np.argmax(np.abs(share - 1 / len(counts))))
    z = stats.norm.ppf(1 - ALPHA / (2 * len(counts)))
    half = z * math.sqrt(share[worst] * (1 - share[worst]) / n)
    return {"name": name, "test": f"chi-square goodness of fit to uniform (dof {len(counts) - 1})",
            "n1": int(n), "n2": 0, "statistic": float(chi2), "p": float(p),
            "effect": "largest deviation of a share from uniform", "effect_value": float(share[worst] - 1 / len(counts)),
            "ci": [float(share[worst] - 1 / len(counts) - half), float(share[worst] - 1 / len(counts) + half)],
            "mde": (3.29 + 0.84) * math.sqrt((1 / len(counts)) * (1 - 1 / len(counts)) / n),
            "mde_unit": "share", "note": note, "table": {"counts": counts.astype(int).tolist()}}


def inclusion(name, counts1, n1, counts2, n2, labels, note=""):
    c1 = np.asarray(counts1, dtype=float)
    c2 = np.asarray(counts2, dtype=float)
    out = {"name": name, "n1": n1, "n2": n2, "effect": "largest inclusion frequency difference (freq1 - freq2)",
           "mde_unit": "frequency", "statistic": float("nan")}
    if n1 == 0 or n2 == 0 or len(c1) == 0:
        out.update(test="none (empty sample)", p=float("nan"), effect_value=float("nan"), ci=[float("nan")] * 2,
                   mde=float("nan"), note=note)
        return out
    f1, f2 = c1 / n1, c2 / n2
    k = len(c1)
    pvals = []
    for a, b in zip(c1, c2):
        if a + b == 0 or a + b == n1 + n2:
            pvals.append(1.0)
        elif max(n1, n2) <= 20000:
            pvals.append(stats.fisher_exact([[int(a), int(n1 - a)], [int(b), int(n2 - b)]])[1])
        else:
            pooled = (a + b) / (n1 + n2)
            z = (a / n1 - b / n2) / math.sqrt(pooled * (1 - pooled) * (1 / n1 + 1 / n2))
            pvals.append(2 * stats.norm.sf(abs(z)))
    worst = int(np.argmax(np.abs(f1 - f2)))
    z = stats.norm.ppf(1 - ALPHA / (2 * k))
    half = z * math.sqrt(f1[worst] * (1 - f1[worst]) / n1 + f2[worst] * (1 - f2[worst]) / n2)
    out.update(test=f"per-item Fisher exact, Bonferroni over {k} items", p=float(min(1.0, k * min(pvals))),
               effect_value=float(f1[worst] - f2[worst]),
               ci=[float(f1[worst] - f2[worst] - half), float(f1[worst] - f2[worst] + half)],
               mde=(3.29 + 0.84) * math.sqrt(0.25 * (1 / n1 + 1 / n2)),
               note=f"largest difference at {labels[worst]} ({f1[worst]:.4g} vs {f2[worst]:.4g}). {note}".strip())
    return out


def always_equal(name, x, n, note=""):
    if n == 0:
        return {"name": name, "test": "none (empty sample)", "n1": 0, "n2": 0, "statistic": float("nan"),
                "p": float("nan"), "effect": "share equal to reference", "effect_value": float("nan"),
                "ci": [float("nan")] * 2, "mde": float("nan"), "mde_unit": "share", "note": note}
    lo = stats.beta.ppf(0.025, x, n - x + 1) if x > 0 else 0.0
    hi = stats.beta.ppf(0.975, x + 1, n - x) if x < n else 1.0
    return {"name": name, "test": "exact binomial against a deterministic reference", "n1": n, "n2": 1,
            "statistic": float("nan"), "p": 1.0 if x == n else 0.0, "effect": "share equal to reference",
            "effect_value": x / n, "ci": [float(lo), float(hi)],
            "mde": 1 - 0.05 ** (1 / n), "mde_unit": "deviation rate", "note": note}


def holm(pvalues):
    p = np.asarray(pvalues, dtype=float)
    adjusted = np.full(len(p), np.nan)
    valid = np.where(~np.isnan(p))[0]
    order = valid[np.argsort(p[valid])]
    m = len(order)
    running = 0.0
    for rank, idx in enumerate(order):
        running = max(running, min(1.0, (m - rank) * p[idx]))
        adjusted[idx] = running
    return adjusted
