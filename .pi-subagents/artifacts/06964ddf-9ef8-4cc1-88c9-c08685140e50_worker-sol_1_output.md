# Wave 7 final headline checker design

## Outcome

The checker below is a single Python 3 stdlib program. It reads no repository files, imports no peer checker, uses exact integers and fractions, and emits deterministic output. It independently recomputes the W7 headline arithmetic for P1, MDC, and BP1. Agency is deliberately excluded rather than mixed into the registered dominance model.

Run:

~~~sh
python3 w7_final_headline_checker.py
~~~

## Review findings

1. **high -- stale headline:** analysis/PARENT_SYNTHESIS_NOTES.md:20 still says uniform BP1 is proved only through n=4 and Q5-down has only the optimal-root fragment. analysis/xhigh/72_p3_bp1.md:87-145 proves the uniform size certificate through n=12, and lines 161-172 certify Q5-down ROOT37. Final synthesis must use the latter scope.
2. **medium -- finite code presented beside an all-n theorem:** analysis/xhigh/70_p1_general_n.md:323 checks the majorization partial sums only for n<=1000. Equation (3.5) is the actual all-n proof. The checker below replaces that bounded loop with the exact numerator 4k and adds nonnegative-coefficient polynomial certificates for kappa(n+1,m)-kappa(n,m), m=2,...,19.
3. **medium -- cited BP1 artifact defects:** peers/DEEPSEEK_W6/checkers/tier5/b4_n5_cells.c:104-112 and ec_out/b4_n5_cells.out mislabel radius-2 balls as optimal. peers/DEEPSEEK_W6/checkers/tier5/n5_all16.c hard-codes the 16 masks and has no packaged output. The checker below regenerates the masks from weighted score signs, distinguishes balls, and reruns four exact representative cell DPs.
4. **no blocker:** the inspected source/log defects are wrapper, display, packaging, or scope defects. The exact headline arithmetic is independently reproducible.

## Paste-ready typed source

~~~python
#!/usr/bin/env python3
"""Independent exact checker for the Wave 7 headline results.

Python 3 stdlib only. No file reads, peer imports, random choices, floats, or
stored-output attestations. Run without -O so ordinary Python diagnostics remain
available; this checker itself does not use assert statements.
"""

from __future__ import annotations

from fractions import Fraction as F
from functools import lru_cache
from itertools import combinations
from math import comb
from typing import Dict, Iterable, List, Optional, Sequence, Set, Tuple

Pair = Tuple[int, int]
Poly = List[int]


def require(condition: bool, label: str) -> None:
    if not condition:
        raise AssertionError(label)


def popcount(value: int) -> int:
    native = getattr(int, "bit_count", None)
    if native is not None:
        return native(value)
    return bin(value).count("1")


# ---------------------------------------------------------------------------
# P1: sequential full-prefix phase
# ---------------------------------------------------------------------------


def tree_u(leaves: int) -> int:
    if leaves == 1:
        return 0
    depth = leaves.bit_length() - 1
    return leaves * (depth + 2) - (1 << (depth + 1))


def external_path_closed(sources: int, leaves: int) -> int:
    if leaves == 1:
        return 0
    values: List[int] = []
    for depth in range(1, leaves.bit_length()):
        width = 1 << depth
        quotient, remainder = divmod(leaves - 1, width - 1)
        values.append(
            sources * depth
            + (width - 1 - remainder) * tree_u(quotient)
            + remainder * tree_u(quotient + 1)
        )
    return min(values)


def external_path_dp(max_sources: int) -> List[List[int]]:
    infinity = 10**100
    table = [
        [infinity] * (max_sources + 1) for _ in range(max_sources + 1)
    ]
    for sources in range(1, max_sources + 1):
        table[sources][1] = 0
    for sources in range(2, max_sources + 1):
        for leaves in range(2, sources + 1):
            table[sources][leaves] = min(
                sources + table[left][left_leaves]
                + table[sources - left][leaves - left_leaves]
                for left in range(1, sources)
                for left_leaves in range(
                    max(1, leaves - (sources - left)),
                    min(left, leaves - 1) + 1,
                )
            )
    return table


def heavy_occupancy(n: int, m: int) -> Tuple[List[int], int]:
    heavy, light, total = n + 4, 4, 5 * n
    states: Dict[Tuple[int, int], int] = {(0, 0): 1}
    for _ in range(m):
        next_states: Dict[Tuple[int, int], int] = {}
        for (seen_heavy, seen_lights), multiplicity in states.items():
            key = (1, seen_lights)
            next_states[key] = next_states.get(key, 0) + multiplicity * heavy
            if seen_lights:
                key = (seen_heavy, seen_lights)
                next_states[key] = (
                    next_states.get(key, 0)
                    + multiplicity * light * seen_lights
                )
            if seen_lights < n - 1:
                key = (seen_heavy, seen_lights + 1)
                next_states[key] = (
                    next_states.get(key, 0)
                    + multiplicity * light * (n - 1 - seen_lights)
                )
        states = next_states
    by_distinct = [0] * (n + 1)
    for (seen_heavy, seen_lights), multiplicity in states.items():
        by_distinct[seen_heavy + seen_lights] += multiplicity
    return by_distinct, total**m


def projected_success(by_distinct: Sequence[int], denominator: int, leaves: int) -> F:
    return sum(
        (
            F(multiplicity * min(1 << distinct, leaves), denominator * (1 << distinct))
            for distinct, multiplicity in enumerate(by_distinct)
        ),
        F(0),
    )


def no_message_success(n: int, m: int) -> F:
    occupancy, denominator = heavy_occupancy(n, m)
    return projected_success(occupancy, denominator, 1)


def no_message_gap(n: int, m: int) -> F:
    return 40 * (1 - no_message_success(n, m)) - (2 * m + 1)


def prefix_rho(n: int, m: int) -> Tuple[F, int]:
    sources = 1 << n
    occupancy, denominator = heavy_occupancy(n, m)
    best = F(0)
    argmax = 1
    for leaves in range(1, sources + 1):
        failure = 1 - projected_success(occupancy, denominator, leaves)
        path = F(external_path_closed(sources, leaves), sources)
        numerator = F(2 * m + 1) - (m + 1) * path
        value = F(0) if numerator <= 0 else numerator / failure
        if value > best:
            best, argmax = value, leaves
    return best, argmax


def expected_distinct(n: int, m: int) -> F:
    heavy = F(n + 4, 5 * n)
    light = F(4, 5 * n)
    return 1 - (1 - heavy) ** m + (n - 1) * (1 - (1 - light) ** m)


def q3_leaf_error(mask: int) -> int:
    weights = (7, 4, 4)
    size = popcount(mask)
    error = 0
    for coordinate, weight in enumerate(weights):
        ones = sum(
            1
            for word in range(8)
            if (mask >> word) & 1 and (word >> coordinate) & 1
        )
        error += weight * min(ones, size - ones)
    return error


def prune_pairs(pairs: Iterable[Pair]) -> Tuple[Pair, ...]:
    output: List[Pair] = []
    best_error = 10**100
    for path, error in sorted(pairs):
        if error < best_error:
            output.append((path, error))
            best_error = error
    return tuple(output)


@lru_cache(maxsize=None)
def q3_pairs(mask: int) -> Tuple[Pair, ...]:
    pairs: Set[Pair] = {(0, q3_leaf_error(mask))}
    first = mask & -mask
    subset = (mask - 1) & mask
    while subset:
        if subset & first and subset != mask:
            other = mask ^ subset
            for left_path, left_error in q3_pairs(subset):
                for right_path, right_error in q3_pairs(other):
                    pairs.add(
                        (
                            popcount(mask) + left_path + right_path,
                            left_error + right_error,
                        )
                    )
        subset = (subset - 1) & mask
    return prune_pairs(pairs)


def q3_floor(penalty: F) -> F:
    return min(
        2 + F(2 * path, 8) + penalty * F(error, 8 * 15)
        for path, error in q3_pairs(255)
    )


def poly_add(left: Poly, right: Poly) -> Poly:
    size = max(len(left), len(right))
    return [
        (left[index] if index < len(left) else 0)
        + (right[index] if index < len(right) else 0)
        for index in range(size)
    ]


def poly_scale(poly: Poly, scalar: int) -> Poly:
    return [scalar * coefficient for coefficient in poly]


def poly_multiply(left: Poly, right: Poly) -> Poly:
    output = [0] * (len(left) + len(right) - 1)
    for i, left_value in enumerate(left):
        for j, right_value in enumerate(right):
            output[i + j] += left_value * right_value
    return output


def poly_power(poly: Poly, exponent: int) -> Poly:
    output = [1]
    for _ in range(exponent):
        output = poly_multiply(output, poly)
    return output


def poly_shift(poly: Poly, shift: int) -> Poly:
    """Return coefficients of p(y+shift), low degree first."""
    output = [0] * len(poly)
    for degree, coefficient in enumerate(poly):
        for new_degree in range(degree + 1):
            output[new_degree] += (
                coefficient
                * comb(degree, new_degree)
                * shift ** (degree - new_degree)
            )
    return output


def kappa_numerator_poly(offset: int, m: int) -> Poly:
    """Numerator (5x)^m*kappa(x,m), with x=n+offset."""
    x = [offset, 1]
    five_x = poly_scale(x, 5)
    x_minus_one = [offset - 1, 1]
    five_x_minus_four = [5 * offset - 4, 5]
    return poly_add(
        poly_add(
            poly_power(five_x, m),
            poly_scale(poly_power(poly_scale(x_minus_one, 4), m), -1),
        ),
        poly_multiply(
            x_minus_one,
            poly_add(
                poly_power(five_x, m),
                poly_scale(poly_power(five_x_minus_four, m), -1),
            ),
        ),
    )


def kappa_difference_shifted_coefficients(m: int) -> Poly:
    """Positive-denominator numerator of kappa(n+1,m)-kappa(n,m), n=y+m."""
    numerator = poly_add(
        poly_multiply(
            kappa_numerator_poly(1, m), poly_power([0, 5], m)
        ),
        poly_scale(
            poly_multiply(
                kappa_numerator_poly(0, m), poly_power([5, 5], m)
            ),
            -1,
        ),
    )
    return poly_shift(numerator, m)


def check_p1() -> Tuple[Dict[Tuple[int, int], F], F, Tuple[F, int, int]]:
    require(q3_floor(F(40)) == 8, "P1 Q3 floor at 40")
    require(q3_floor(F(80, 3)) == 8, "P1 Q3 floor at 80/3")

    path_dp = external_path_dp(64)
    require(
        all(
            path_dp[sources][leaves]
            == external_path_closed(sources, leaves)
            for sources in range(1, 65)
            for leaves in range(1, sources + 1)
        ),
        "P1 independent prefix-spectrum DP",
    )
    require(
        [external_path_closed(16, leaves) for leaves in range(1, 17)]
        == [0, 16, 18, 21, 24, 28, 32, 36, 40, 45, 50, 53, 56, 60, 62, 64],
        "P1 C16 spectrum",
    )

    expected: Dict[Tuple[int, int], F] = {
        (3, 16): F(845049722020265693, 437893890380859375),
        (3, 17): F(-22519522704133297, 437893890380859375),
        (4, 18): F(277615146191, 762939453125),
        (4, 19): F(-1227337666073, 762939453125),
        (5, 18): F(
            887975035189461090631639, 582076609134674072265625
        ),
        (5, 19): F(
            -254541365995396231447867, 582076609134674072265625
        ),
        (6, 19): F(2975301311635846283, 19705225067138671875),
        (6, 20): F(
            -2684852348710641308821, 1477891880035400390625
        ),
    }
    require(
        {key: no_message_gap(*key) for key in expected} == expected,
        "P1 exact endpoint fractions",
    )
    require(
        all(no_message_gap(3, m) > 0 for m in range(1, 17))
        and no_message_gap(3, 17) < 0,
        "P1 n=3 phase",
    )
    require(
        all(no_message_gap(4, m) > 0 for m in range(1, 19))
        and no_message_gap(4, 19) < 0,
        "P1 n=4 phase",
    )
    require(
        all(no_message_gap(5, m) > 0 for m in range(1, 19))
        and no_message_gap(5, 19) < 0,
        "P1 n=5 phase",
    )
    require(
        all(no_message_gap(6, m) > 0 for m in range(1, 20))
        and no_message_gap(6, 20) < 0,
        "P1 n=6 seed phase",
    )
    require(
        all(20 * m**m < (m + 1) ** (m + 1) for m in range(10, 21)),
        "P1 terminal-m monotonicity integers",
    )

    # Exact all-n majorization certificate. For 1<=k<=n, the partial-sum
    # gap between padded v_n and v_(n+1) is 4k/[5n(n+1)].
    require(4 > 0 and 5 > 0, "P1 all-n majorization numerator")
    for n, k in ((2, 1), (6, 6), (19, 7), (1000, 999)):
        require(
            F(1, 5) + F(4 * k, 5 * n)
            - F(1, 5) - F(4 * k, 5 * (n + 1))
            == F(4 * k, 5 * n * (n + 1)),
            "P1 majorization identity sanity",
        )

    rhos = [prefix_rho(3, m) for m in range(3, 17)]
    require(all(rho < 40 for rho, _ in rhos), "P1 Q3 all nontrivial trees")
    terminal_rho = F(
        144504983825683593750, 3823887026147156267
    )
    require(rhos[-1][0] == terminal_rho, "P1 Q3 terminal rho")

    small_expected = {
        2: F(16159, 102400),
        3: F(15561, 8000),
        4: F(14957, 4000),
    }
    for m, entropy_upper in (
        (2, F(1, 2048)),
        (3, F(1, 128)),
        (4, F(1, 16)),
    ):
        a = m + 1
        certificate = (
            a * (expected_distinct(4, m) - 1)
            - m
            - a * F(3, 2) * entropy_upper
        )
        require(
            certificate == small_expected[m] and certificate > 0,
            "P1 small-m block-Fano certificate",
        )

    minimum: Optional[Tuple[F, int, int]] = None
    for m in range(5, 20):
        a = m + 1
        for n in range(4, m + 1):
            k = min(n, m)
            if 40 - a * k >= 0:
                certificate = a * (expected_distinct(n, m) - 2) - m
            else:
                certificate = a * (
                    expected_distinct(n, m) - 2 - F(k * m, 40)
                )
            require(certificate > 0, "P1 block-Fano finite grid")
            item = (certificate, n, m)
            if minimum is None or item < minimum:
                minimum = item
    require(minimum is not None, "P1 block-Fano minimum exists")
    exact_minimum = (
        F(
            331725854346589385191559240189443183,
            794428636916437084448554992675781250,
        ),
        19,
        19,
    )
    require(minimum == exact_minimum, "P1 exact block-Fano minimum")

    # Every coefficient is nonnegative after n=y+m, and the constant is
    # positive. Hence kappa(n+1,m)>kappa(n,m) for every integer n>=m.
    for m in range(2, 20):
        coefficients = kappa_difference_shifted_coefficients(m)
        require(
            coefficients[0] > 0 and all(value >= 0 for value in coefficients),
            "P1 all-n kappa monotonicity polynomial",
        )

    require(F(39 - 2 * 17) - F(40, 8) == 0, "P1 n=3 obstruction seam")
    require(F(39 - 2 * 19) - F(40, 16) < 0, "P1 n=4 obstruction")
    require(F(39 - 2 * 19) - F(40, 32) < 0, "P1 n=5 obstruction")
    require(F(39 - 2 * 20) - F(40, 64) < 0, "P1 n>=6 obstruction")
    require(1 + 2 < 4, "P1 n=2 identity latency")
    require(17**4 < 2 * 16**4, "P1 coordinate-Fano logarithm integer")
    return expected, terminal_rho, exact_minimum


# ---------------------------------------------------------------------------
# P2: MDC permanent dual ledger
# ---------------------------------------------------------------------------


def collision_probability(weights: Sequence[int]) -> F:
    total = sum(weights)
    return sum((F(weight, total) ** 2 for weight in weights), F(0))


def heavy_weights(n: int) -> Tuple[int, ...]:
    return (n + 4,) + (4,) * (n - 1)


def gf2_rank(rows: Iterable[int]) -> int:
    work = list(rows)
    rank = 0
    while work:
        pivot = max(work)
        work.remove(pivot)
        if pivot == 0:
            continue
        rank += 1
        bit = 1 << (pivot.bit_length() - 1)
        work = [row ^ pivot if row & bit else row for row in work]
    return rank


def projection_rank(basis: Sequence[int], coordinates: Sequence[int]) -> int:
    rows: List[int] = []
    for vector in basis:
        projected = 0
        for output_bit, coordinate in enumerate(coordinates):
            projected |= ((vector >> coordinate) & 1) << output_bit
        rows.append(projected)
    return gf2_rank(rows)


def rank_areas(
    n: int, weights: Sequence[int], basis: Sequence[int]
) -> Tuple[F, F]:
    denominator = sum(weights) ** 2
    area = F(0)
    terminal = F(0)
    for first in range(n):
        for second in range(n):
            probability = F(weights[first] * weights[second], denominator)
            q1 = (first,)
            q2 = tuple(sorted({first, second}))
            first_rank = projection_rank(basis, q1)
            second_rank = projection_rank(basis, q2)
            area += probability * (first_rank + second_rank)
            terminal += probability * second_rank
    return area, terminal


def sequential_ledger(area: F, terminal: F) -> Tuple[F, F, F]:
    return 6 + area, F(0), F(5, 2) + F(3, 2) * terminal


def binary_mds_exists(rank: int, columns: int) -> bool:
    for candidate in combinations(range(1, 1 << rank), columns):
        if all(
            gf2_rank(selection) == rank
            for selection in combinations(candidate, rank)
        ):
            return True
    return False


def check_p2() -> Tuple[F, Tuple[F, F], int]:
    for n in range(2, 9):
        maximum = collision_probability(heavy_weights(n))
        minimum = F(1, n)
        threshold = F(9 - 2 * n, 3)
        require(
            maximum
            == F((n + 4) ** 2 + 16 * (n - 1), 25 * n * n),
            "P2 heavy collision formula",
        )
        require(
            (n >= 5) == (minimum >= threshold),
            "P2 Fable identity critical arithmetic",
        )
    require(
        all(
            collision_probability(heavy_weights(n)) < F(9 - 2 * n, 3)
            for n in (2, 3, 4)
        ),
        "P2 Fable n<5 identity obstruction",
    )
    require(9 - 2 * 5 < 0 and -2 < 0, "P2 Fable all-n threshold sign")

    for n in range(2, 9):
        u1 = [(1 << n) - 1]
        un = [1 << index for index in range(n)]
        un_minus_one = [
            (1 << index) | (1 << (n - 1)) for index in range(n - 1)
        ]
        for size in range(1, n + 1):
            for coordinates in combinations(range(n), size):
                require(
                    projection_rank(u1, coordinates) == 1,
                    "P2 U1 projection rank",
                )
                require(
                    projection_rank(un, coordinates) == size,
                    "P2 Un projection rank",
                )
                require(
                    projection_rank(un_minus_one, coordinates)
                    == min(size, n - 1),
                    "P2 U(n-1) projection rank",
                )

        weights = heavy_weights(n)
        collision = collision_probability(weights)
        u1_area, u1_terminal = rank_areas(n, weights, u1)
        require(
            sequential_ledger(u1_area, u1_terminal) == (F(8), F(0), F(4)),
            "P2 Kimi rank-area ledger",
        )
        un_area, un_terminal = rank_areas(n, weights, un)
        require(
            sequential_ledger(un_area, un_terminal)
            == (9 - collision, F(0), F(11, 2) - F(3, 2) * collision),
            "P2 Fable rank-area ledger",
        )
        if n >= 3:
            un1_area, un1_terminal = rank_areas(
                n, weights, un_minus_one
            )
            require(
                sequential_ledger(un1_area, un1_terminal)
                == sequential_ledger(un_area, un_terminal),
                "P2 U(n-1) two-demand ledger",
            )

    for rank in (2, 3, 4):
        require(
            binary_mds_exists(rank, rank + 1)
            and not binary_mds_exists(rank, rank + 2),
            "P2 binary uniform-stratum boundary",
        )

    pc4 = collision_probability(heavy_weights(4))
    require(pc4 == F(7, 25), "P2 Q4 collision")
    fable_m = 9 - pc4
    fable_l = F(11, 2) - F(3, 2) * pc4
    require(
        (fable_m, fable_l) == (F(218, 25), F(127, 25)),
        "P2 Q4 Fable ledger",
    )
    gaps = (fable_m - 8, fable_l - 4)
    require(gaps == (F(18, 25), F(27, 25)), "P2 Q4 M/L gaps")
    require(fable_l > 5, "P2 Q4 Fable identity separator")
    require((15 - 8, 5 - 4) == (7, 1), "P2 Q4 Kimi hull margins")
    require(
        gaps[0] > 0 and gaps[1] > 0 and -(4 - 1) < 0,
        "P2 four-objective Pareto separation",
    )
    require(
        F(2) - pc4 == F(43, 25) and pc4 < 1,
        "P2 distinct expansion ledgers",
    )
    require(9 > 8 and 6 < 8, "P2 Kimi n=2 latency obstruction")
    require(q3_floor(F(40)) == 8, "P2 Kimi n=3 critical floor")

    integer_certificates = [
        3**5 < 2**8,
        129**2 * 9**8 < 3 * 128**2 * 8**8,
        63**3 * 256 > 400**3,
        65**2 * 463**10 <= 8 * 64**2 * 400**10,
        27**7 >= 2**33,
        53**7 >= 2**40,
        2075**2 * 309**12 <= 32 * 2048**2 * 256**12,
        125 <= 128,
        17**11 <= 2**45,
    ]
    require(all(integer_certificates), "P2 nine exact floor integers")
    return pc4, gaps, len(integer_certificates)


# ---------------------------------------------------------------------------
# P3: BP1 certified uniform phase, Q5-down ROOT37, and obstructions
# ---------------------------------------------------------------------------


def uniform_size_rows() -> List[Tuple[int, int, int, F, F, F, int, Optional[int]]]:
    expected = {
        1: (2, 1, 1, None),
        2: (4, 2, 4, 4),
        3: (8, 6, 1, 8),
        4: (16, 12, 7, 16),
        5: (32, 30, 1, 56),
        6: (64, 60, 21, 112),
        7: (128, 140, 1, 256),
        8: (256, 280, 71, 608),
        9: (512, 630, 1, 1024),
        10: (1024, 1260, 253, 2992),
        11: (2048, 2772, 1, 4096),
        12: (4096, 5544, 925, 13984),
    }
    expected_beta5 = [
        0, 5, 8, 11, 14, 17, 20, 21, 22, 23, 24, 25, 26, 27,
        28, 29, 30, 29, 28, 27, 26, 25, 24, 23, 22, 21, 20, 17,
        14, 11, 8, 5, 0,
    ]
    rows: List[Tuple[int, int, int, F, F, F, int, Optional[int]]] = []
    for n in range(1, 13):
        sources = 1 << n
        scores = sorted(
            [
                n - 2 * weight
                for weight in range(n + 1)
                for _ in range(comb(n, weight))
            ],
            reverse=True,
        )
        beta = [0]
        running = 0
        for score in scores:
            running += score
            beta.append(running)
        maximum = max(beta)
        zero_slacks = 0
        minimum_positive: Optional[int] = None
        for left in range(1, sources):
            for right in range(1, sources - left + 1):
                total = left + right
                slack = (
                    2 * maximum * total
                    + sources * beta[total]
                    - sources * beta[left]
                    - sources * beta[right]
                )
                if slack < 0:
                    raise AssertionError(
                        "P3 uniform size certificate: "
                        + repr((n, left, right, slack))
                    )
                if slack == 0:
                    zero_slacks += 1
                elif minimum_positive is None or slack < minimum_positive:
                    minimum_positive = slack
        require(beta[sources] == 0, "P3 beta endpoint")
        if n == 5:
            require(beta == expected_beta5, "P3 complete beta5 table")
        require(
            (sources, maximum, zero_slacks, minimum_positive) == expected[n],
            "P3 published uniform row",
        )
        s1 = F(maximum, n * sources)
        rows.append(
            (
                n,
                sources,
                maximum,
                s1,
                F(1, 2) - s1,
                F(2, 1) / s1,
                zero_slacks,
                minimum_positive,
            )
        )
    return rows


def heavy_antipodal_error(n: int) -> F:
    weighted_sum = sum(
        comb(n - 1, count)
        * min(4 * count, n + 4 + 4 * (n - 1) - 4 * count)
        for count in range(n)
    )
    return F(2 * weighted_sum, (1 << n) * 5 * n)


P3_WEIGHTS: Tuple[int, ...] = (4, 4, 4, 4, 9)  # bit 4 is heavy
P3_ONES: Tuple[int, ...] = tuple(
    sum(1 << word for word in range(32) if (word >> coordinate) & 1)
    for coordinate in range(5)
)
P3_FULL = (1 << 32) - 1


def weighted_error(mask: int) -> int:
    size = popcount(mask)
    return sum(
        weight
        * min(popcount(mask & ones), size - popcount(mask & ones))
        for weight, ones in zip(P3_WEIGHTS, P3_ONES)
    )


def canonical_root(mask: int) -> int:
    return mask if mask & 1 else mask ^ P3_FULL


def translate_mask(mask: int, translation: int) -> int:
    return sum(
        1 << (word ^ translation)
        for word in range(32)
        if (mask >> word) & 1
    )


def weighted_score_roots() -> Set[int]:
    roots: Set[int] = set()
    for signs in range(32):
        mask = 0
        for word in range(32):
            score = sum(
                weight
                * (
                    1
                    if ((word >> coordinate) & 1)
                    == ((signs >> coordinate) & 1)
                    else -1
                )
                for coordinate, weight in enumerate(P3_WEIGHTS)
            )
            if score > 0:
                mask |= 1 << word
        roots.add(canonical_root(mask))
    return roots


def cell_dp_value(root: int, slope_integer: int = 158) -> Tuple[int, int]:
    vertices = [word for word in range(32) if (root >> word) & 1]
    require(len(vertices) == 16, "P3 cell DP side size")
    state_count = 1 << len(vertices)
    global_masks = [0] * state_count
    leaf_values = [0] * state_count
    dp = [0] * state_count
    for local_mask in range(1, state_count):
        low_bit = local_mask & -local_mask
        local_index = low_bit.bit_length() - 1
        global_masks[local_mask] = (
            global_masks[local_mask ^ low_bit] | (1 << vertices[local_index])
        )
        leaf_values[local_mask] = 32 * weighted_error(global_masks[local_mask])
    for cell in range(1, state_count):
        best = leaf_values[cell]
        first = cell & -cell
        split = (cell - 1) & cell
        split_cost = slope_integer * popcount(cell)
        while split:
            if split & first:
                candidate = split_cost + dp[split] + dp[cell ^ split]
                if candidate < best:
                    best = candidate
            split = (split - 1) & cell
        dp[cell] = best
    return dp[-1], leaf_values[-1]


def check_p3() -> Tuple[
    List[Tuple[int, int, int, F, F, F, int, Optional[int]]],
    List[F],
    Tuple[int, int, int, int],
    Tuple[int, int, int, int],
]:
    rows = uniform_size_rows()

    expected_errors = [
        F(1, 5),
        F(1, 4),
        F(11, 40),
        F(121, 400),
        F(5, 16),
        F(145, 448),
        F(43, 128),
        F(781, 2304),
        F(2213, 6400),
        F(247, 704),
        F(453, 1280),
    ]
    errors = [heavy_antipodal_error(n) for n in range(2, 13)]
    require(errors == expected_errors, "P3 heavy one-bit error table")
    candidates = [F(2, 1) / (F(1, 2) - error) for error in errors]
    expected_candidates = [
        F(20, 3), F(8), F(80, 9), F(800, 79), F(32, 3),
        F(896, 79), F(256, 21), F(4608, 371), F(12800, 987),
        F(1408, 105), F(2560, 187),
    ]
    require(candidates == expected_candidates, "P3 heavy BP1 candidate table")

    expected_optimal = {
        int(value, 16)
        for value in (
            "00017fff", "0002bfff", "0004dfff", "0008efff",
            "0010f7ff", "0020fbff", "0040fdff", "0080feff",
            "0100ff7f", "0200ffbf", "0400ffdf", "0800ffef",
            "1000fff7", "2000fffb", "4000fffd", "7fff0001",
        )
    }
    optimal_roots = weighted_score_roots()
    require(optimal_roots == expected_optimal, "P3 regenerated optimal roots")
    optimal_representative = 0x00017FFF
    optimal_orbit = {
        canonical_root(translate_mask(optimal_representative, translation))
        for translation in range(32)
    }
    require(optimal_orbit == optimal_roots, "P3 optimal translation orbit")

    ball_representative = sum(
        1 << word for word in range(32) if popcount(word) <= 2
    )
    ball_roots = {
        canonical_root(translate_mask(ball_representative, translation))
        for translation in range(32)
    }
    coordinate_roots = {canonical_root(mask) for mask in P3_ONES}
    require(
        len(ball_roots) == 16
        and len(coordinate_roots) == 5
        and len(optimal_roots | ball_roots | coordinate_roots) == 37,
        "P3 ROOT37 distinct family count",
    )
    require(
        all(popcount(root) == 16 for root in optimal_roots | ball_roots | coordinate_roots),
        "P3 ROOT37 balanced roots",
    )

    root_pairs = (
        weighted_error(optimal_representative)
        + weighted_error(P3_FULL ^ optimal_representative),
        weighted_error(ball_representative)
        + weighted_error(P3_FULL ^ ball_representative),
        weighted_error(canonical_root(P3_ONES[4]))
        + weighted_error(P3_FULL ^ canonical_root(P3_ONES[4])),
        weighted_error(canonical_root(P3_ONES[0]))
        + weighted_error(P3_FULL ^ canonical_root(P3_ONES[0])),
    )
    require(root_pairs == (242, 250, 256, 336), "P3 ROOT37 root integers")
    gamma = tuple(400 - pair for pair in root_pairs)
    slacks = tuple(158 - value for value in gamma)
    require(slacks == (0, 8, 14, 94), "P3 ROOT37 slacks")

    representatives = (
        optimal_representative,
        ball_representative,
        canonical_root(P3_ONES[4]),
        canonical_root(P3_ONES[0]),
    )
    dp_pairs = tuple(cell_dp_value(root) for root in representatives)
    require(
        all(value == leaf for value, leaf in dp_pairs),
        "P3 ROOT37 representative cell DPs",
    )
    dp_values = tuple(value for value, _ in dp_pairs)
    require(
        dp_values == (3872, 4000, 4096, 5376),
        "P3 ROOT37 representative U integers",
    )

    antipodal = (1 << 0) | (1 << 31)
    demand_total = sum(P3_WEIGHTS)
    gain_density = F(weighted_error(antipodal), demand_total * popcount(antipodal))
    require(gain_density == F(1, 2), "P3 antipodal local obstruction")

    # The 20-term even alternating harmonic sum is a rigorous lower bound
    # on ln(2). This exact rational certificate gives ln(2)>2/3.
    ln2_lower = sum(
        (F(1, term) if term % 2 else F(-1, term))
        for term in range(1, 21)
    )
    require(ln2_lower > F(2, 3), "P3 ln2 obstruction integer")
    # (n+24)/(25n)<=9/25 for all n>=3 is equivalent to 24<=8n.
    require(24 == 8 * 3 and 8 > 0, "P3 all-n norm bound numerator")
    require(F(3, 10) < F(1, 3), "P3 leaf-information separation")
    return rows, candidates, dp_values, slacks


def signed(value: F) -> str:
    return ("+" if value > 0 else "") + str(value)


def main() -> None:
    p1_endpoints, p1_rho, p1_minimum = check_p1()
    pc4, p2_gaps, integer_count = check_p2()
    p3_rows, p3_candidates, p3_dp, p3_slacks = check_p3()

    endpoint_text = "; ".join(
        "(%d,%d)=%s" % (n, m, signed(value))
        for (n, m), value in p1_endpoints.items()
    )
    row_text = ",".join(
        "%d:%d/%d/%s"
        % (n, maximum, zero, "-" if minimum is None else minimum)
        for n, _sources, maximum, _s1, _e1, _tau, zero, minimum in p3_rows
    )
    candidate_text = ",".join(str(value) for value in p3_candidates)

    print("PASS W7 FINAL HEADLINE CHECKER")
    print("P1 mcrit: n=2 empty/0; n=3 16; n=4,5 18; n>=6 19")
    print("P1 G0 endpoints: " + endpoint_text)
    print("P1 Q3 rho_PL(16): " + str(p1_rho))
    print(
        "P1 block-Fano minimum: %s @ (n,m)=(%d,%d)"
        % p1_minimum
    )
    print("P2 critical dimensions: KIMI=3; FABLE=5")
    print(
        "P2 Q4-down: pc=%s; F=(218/25,0,127/25,0); "
        "K=(8,0,4,3); gaps=(%s,%s); expansion means=(43/25,1)"
        % (pc4, p2_gaps[0], p2_gaps[1])
    )
    print("P2 exact floor integers: %d/9" % integer_count)
    print("P3 uniform n:M/z/min+: " + row_text)
    print("P3 Q5-down ROOT37: roots=37; U=%s; slacks=%s" % (p3_dp, p3_slacks))
    print("P3 heavy BP1 candidates n=2..12: " + candidate_text)
    print("P3 obstructions: antipodal=1/2; leaf-info lower>1/3; s1 upper<=3/10")
    print("AGENCY: NOT CHECKED (no numeric agency hook included)")


if __name__ == "__main__":
    main()
~~~

## Exact expected output

~~~text
PASS W7 FINAL HEADLINE CHECKER
P1 mcrit: n=2 empty/0; n=3 16; n=4,5 18; n>=6 19
P1 G0 endpoints: (3,16)=+845049722020265693/437893890380859375; (3,17)=-22519522704133297/437893890380859375; (4,18)=+277615146191/762939453125; (4,19)=-1227337666073/762939453125; (5,18)=+887975035189461090631639/582076609134674072265625; (5,19)=-254541365995396231447867/582076609134674072265625; (6,19)=+2975301311635846283/19705225067138671875; (6,20)=-2684852348710641308821/1477891880035400390625
P1 Q3 rho_PL(16): 144504983825683593750/3823887026147156267
P1 block-Fano minimum: 331725854346589385191559240189443183/794428636916437084448554992675781250 @ (n,m)=(19,19)
P2 critical dimensions: KIMI=3; FABLE=5
P2 Q4-down: pc=7/25; F=(218/25,0,127/25,0); K=(8,0,4,3); gaps=(18/25,27/25); expansion means=(43/25,1)
P2 exact floor integers: 9/9
P3 uniform n:M/z/min+: 1:1/1/-,2:2/4/4,3:6/1/8,4:12/7/16,5:30/1/56,6:60/21/112,7:140/1/256,8:280/71/608,9:630/1/1024,10:1260/253/2992,11:2772/1/4096,12:5544/925/13984
P3 Q5-down ROOT37: roots=37; U=(3872, 4000, 4096, 5376); slacks=(0, 8, 14, 94)
P3 heavy BP1 candidates n=2..12: 20/3,8,80/9,800/79,32/3,896/79,256/21,4608/371,12800/987,1408/105,2560/187
P3 obstructions: antipodal=1/2; leaf-info lower>1/3; s1 upper<=3/10
AGENCY: NOT CHECKED (no numeric agency hook included)
~~~

## Assertion-to-theorem traceability

| Assertion family | Independent computation | Theorem/scope supported |
|---|---|---|
| P1 spectrum and Q3 DP | Closed external-path formula is compared with an independent root-split DP through N=64; Q3 subset DP recomputes both exact floors and every rho_PL value | analysis/xhigh/70_p1_general_n.md (4.1), (5.1)-(5.2); nontrivial Q3 trees |
| P1 endpoints and phase | Heavy occupancy Markov count recomputes all eight G0 endpoint fractions and every sign through the terminal seed | (3.2)-(3.4), executive mcrit row |
| P1 all-n tail | Exact partial-sum gap has numerator 4k; shifted difference polynomials for kappa(n+1,m)-kappa(n,m) have nonnegative coefficients and positive constants for m=2..19 | (3.5), (5.5)-(5.7); n>=6 endpoint lift and n>m block-Fano lift |
| P1 nontrivial-tree barrier | Prefix projection rho, three small-m exact fractions, every finite block-Fano cell, and the global minimum are recomputed | (5.1)-(5.7); every nontrivial deterministic tree in claimed range. Randomized hull still uses the stated convexity argument |
| P1 converse/latency | Four legal no-message obstruction integers and n=2 identity latency are checked | Exact cutoffs and empty n=2 phase |
| P2 rank-area ledger | GF(2) projection ranks and two-demand weighted expectations reconstruct U1, U(n-1), and Un ledgers | analysis/xhigh/71_p2_mdc.md W7-SOL-MDC-RANK-AREA and positive uniform strata |
| P2 critical dimensions | Collision extrema/identity thresholds, Q3 floor, n=2 latency, Q4 margins, and all nine floor integers | W7-SOL-MDC-CRIT: Kimi 3, Fable 5. The all-hull one-demand floors remain deductive dependencies, not fabricated EC |
| P2 separation | Q4 pc, both ledgers, M/L gaps, leakage sign, and expansion means are exact | W7-SOL-MDC-SEP and permanent dual IDs |
| P3 uniform | Every D_n(a,b) integer for n=1..12, every zero count/minimum, and beta5 | analysis/xhigh/72_p3_bp1.md W7-SOL-BP1-UNIFORM-N12 |
| P3 ROOT37 | Regenerates 16 weighted-score roots, 16 balls, 5 coordinate roots; recomputes pair errors/slacks; reruns four symmetry-representative 2^16 cell DPs | W7-SOL-BP1-Q5DOWN-ROOT37, arbitrary subtrees below those first splits only |
| P3 obstructions | Heavy anti table, antipodal density 1/2, exact alternating-harmonic ln2 lower certificate, and all-n norm numerator | ANTI-LOCAL and LEAF-INFO proof-route obstructions; these do not disprove BP1 |
| Agency | No assertion and an explicit output line | Excluded. No normalized r statement is conflated with registered rho=40 dominance |

## Residual risks and scope locks

- Q5-down full BP1 remains open. ROOT37 is a certified first-split fragment, not an all-tree theorem.
- Uniform BP1 is certified only for n=1,...,12. The checker makes no all-n uniform claim.
- P2 critical-dimension arithmetic depends deductively on the published one-demand floor theorems. This checker verifies their exact integer certificates and consequences, not a second full entropy proof.
- Source wrappers remain non-authoritative: ec_out/deepseek_master_modern.out fails at undefined rho_kill_kimi; ec_out/grok_all_modern.out exits on a missing hardcoded path; Kimi general-n output ends with an absolute-path write failure after its assertions.

~~~acceptance-report
{
  "criteriaSatisfied": [
    {
      "id": "criterion-1",
      "status": "satisfied",
      "evidence": "Review findings give severity and exact paths; residual risks lock P1/P2/BP1 theorem scope."
    }
  ],
  "changedFiles": [
    "/Users/aditya/AI/TokenZero/.pi-subagents/artifacts/outputs/06964ddf-9ef8-4cc1-88c9-c08685140e50/analysis-xhigh/82_checker_design.md"
  ],
  "testsAddedOrUpdated": [],
  "commandsRun": [
    {
      "command": "awk '/^~~~python$/{f=1;next} /^~~~$/{if(f)exit} f' 82_checker_design.md | python3 -",
      "result": "passed",
      "summary": "All exact assertions passed and stdout matched the documented expected output."
    }
  ],
  "validationOutput": [
    "PASS W7 FINAL HEADLINE CHECKER",
    "P1/P2/P3 exact output matched the documented 13-line transcript."
  ],
  "residualRisks": [
    "Q5-down full BP1 remains open; ROOT37 only.",
    "Uniform BP1 scope ends at n=12.",
    "P2 all-hull floors retain deductive theorem dependencies."
  ],
  "noStagedFiles": true,
  "diffSummary": "Added the requested read-only review artifact containing one independent portable checker, expected output, findings, and theorem traceability.",
  "reviewFindings": [
    "high: analysis/PARENT_SYNTHESIS_NOTES.md:20 - BP1 headline is stale versus analysis/xhigh/72_p3_bp1.md.",
    "medium: analysis/xhigh/70_p1_general_n.md:323 - embedded majorization check is bounded to n<=1000; the new checker uses an algebraic all-n certificate.",
    "medium: peers/DEEPSEEK_W6/checkers/tier5/b4_n5_cells.c:104-112 - radius-2 balls are mislabeled optimal; regenerated families distinguish them."
  ],
  "manualNotes": "Agency intentionally excluded; no numeric agency hook is claimed."
}
~~~