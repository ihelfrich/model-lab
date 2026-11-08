"""Generate the three built-in example datasets.

Every file in this folder is synthetic. The generating mechanisms below are the
whole truth about them: there is no upstream source, no licence to clear and no
real community, household or gemstone behind any row. The shapes imitate the
course datasets the lessons were first written around (income by age and year,
diamond prices from carat and dimensions, community crime rates from
demographics) so that the lesson text still makes sense, but every number here
was drawn from the distributions written in this file.

Run with any Python 3.10+ interpreter; only the standard library is used, so
the output is reproducible byte for byte from the seeds alone:

    python examples/generate.py
"""
from __future__ import annotations

import csv
import math
import random
from pathlib import Path

HERE = Path(__file__).resolve().parent


def write(name: str, header: list[str], rows: list[list[object]]) -> None:
    with (HERE / name).open("w", newline="", encoding="utf-8") as f:
        w = csv.writer(f, lineterminator="\n")
        w.writerow(header)
        w.writerows(rows)


def income(n: int = 3000, seed: int = 20261107) -> None:
    """Income (thousands) against age and survey year.

    Truth: a concave age profile peaking near 50, a linear year trend, and a
    heteroskedastic error that grows with age. Age and year are independent.
    """
    rng = random.Random(seed)
    rows = []
    for _ in range(n):
        age = rng.randint(18, 70)
        year = rng.randint(2000, 2012)
        mean = 30.0 + 2.6 * (age - 18) - 0.038 * (age - 18) ** 2 + 1.1 * (year - 2000)
        sd = 6.0 + 0.18 * (age - 18)
        rows.append([age, year, round(max(5.0, rng.gauss(mean, sd)), 6)])
    write("income.csv", ["age", "year", "income"], rows)


def diamonds(n: int = 1500, seed: int = 20261108) -> None:
    """Price against carat, depth, table and the three dimensions.

    Truth: log price is linear in log carat with slope 1.7; x, y, z are
    deterministic functions of carat plus small measurement noise, so they are
    nearly collinear with carat and with each other; depth and table are
    uninformative. Prices are rounded to whole units.
    """
    rng = random.Random(seed)
    rows = []
    for _ in range(n):
        carat = round(math.exp(rng.gauss(-0.45, 0.55)), 2)
        carat = min(max(carat, 0.2), 3.0)
        depth = round(rng.gauss(61.8, 1.4), 1)
        table = round(rng.gauss(57.5, 2.2), 0)
        side = (carat / 0.0061) ** (1 / 3)
        x = round(side * rng.gauss(1.0, 0.012), 2)
        y = round(side * rng.gauss(1.0, 0.012), 2)
        z = round(side * depth / 100 * rng.gauss(1.0, 0.015), 2)
        price = int(round(math.exp(8.45 + 1.7 * math.log(carat) + rng.gauss(0, 0.22))))
        rows.append([carat, depth, int(table), x, y, z, price])
    write("diamonds.csv", ["carat", "depth", "table", "x", "y", "z", "price"], rows)


def communities(n: int = 90, seed: int = 20261109) -> None:
    """Violent crimes per 100,000 against eight community descriptors.

    Truth: the rate rises with unemployment, vacancy and density and falls
    with income and education; population and household size are included
    as near-irrelevant predictors. Ninety rows keep the validation sample
    small on purpose, which is the point of the lesson.
    """
    rng = random.Random(seed)
    rows = []
    for _ in range(n):
        population = int(round(math.exp(rng.gauss(10.0, 0.8))))
        household = round(rng.gauss(2.55, 0.22), 2)
        urban = round(min(100.0, max(0.0, rng.gauss(85, 20))), 2)
        income = int(round(rng.gauss(36000, 7500)))
        bs = round(min(60.0, max(3.0, rng.gauss(22, 8))), 2)
        unemployed = round(min(20.0, max(1.0, rng.gauss(5.5, 1.8))), 2)
        vacant = round(min(10.0, max(0.1, rng.gauss(1.4, 0.7))), 2)
        density = round(math.exp(rng.gauss(6.9, 0.7)), 1)
        log_rate = (
            4.9
            + 0.12 * (unemployed - 5.5)
            + 0.25 * (vacant - 1.4)
            + 0.30 * (math.log(density) - 6.9)
            - 0.000035 * (income - 36000)
            - 0.02 * (bs - 22)
            + rng.gauss(0, 0.35)
        )
        rate = round(math.exp(log_rate), 2)
        rows.append([population, household, urban, income, bs, unemployed, vacant, density, rate])
    write(
        "florida.csv",
        [
            "population",
            "household.size",
            "pct.urban",
            "med.income",
            "pct.bs.ormore",
            "pct.unemployed",
            "pct.house.vacant",
            "pop.density",
            "violentcrimes.perpop",
        ],
        rows,
    )


if __name__ == "__main__":
    income()
    diamonds()
    communities()
