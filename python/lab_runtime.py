"""Transparent standard-library regression teaching tools, not sklearn parity.

Same model definitions as the Rust lab. Python and Rust RNGs differ, so their
partitions, sampled rows, chosen tied splits and fitted predictions may differ.
"""
import csv
import json
import math
import os
import random
import time


def load_data(path):
    with open(path, newline="", encoding="utf-8") as handle:
        reader = csv.reader(handle)
        names = next(reader)
        rows = [[float(value) for value in row] for row in reader]
    return [row[:-1] for row in rows], [row[-1] for row in rows], names[:-1]


def split_rows(n, validation_fraction, seed):
    rows = list(range(n))
    random.Random(seed).shuffle(rows)
    count = min(n - 2, max(1, math.floor(n * validation_fraction + 0.5)))
    return rows[count:], rows[:count]


def mean(values):
    return sum(values) / len(values)


def fit_tree(X, y, rows, max_depth, min_leaf, rng, max_features=None):
    """CART regression: greedy numeric SSE splits, leaf = sampled mean."""
    center = mean([y[i] for i in rows])
    total_sse = sum((y[i] - center) ** 2 for i in rows)
    node = {"value": center, "n": len(rows), "split": None}
    if max_depth == 0 or len(rows) < 2 * min_leaf or total_sse == 0:
        return node
    features = list(range(len(X[0])))
    if max_features is not None and max_features < len(features):
        features = rng.sample(features, max_features)
    best_cost = total_sse
    best = None
    for feature in features:
        order = sorted(rows, key=lambda i: X[i][feature])
        centered_sum = sum(y[i] - center for i in order)
        left_sum = left_sq = 0.0
        for k in range(1, len(order)):
            value = y[order[k - 1]] - center
            left_sum += value
            left_sq += value * value
            if k < min_leaf or len(order) - k < min_leaf:
                continue
            if X[order[k - 1]][feature] == X[order[k]][feature]:
                continue
            right_sum = centered_sum - left_sum
            cost = max(0.0, left_sq - left_sum**2 / k)
            cost += max(0.0, total_sse - left_sq - right_sum**2 / (len(order) - k))
            if cost < best_cost - 1e-12 * max(total_sse, 1e-12):
                a, b = X[order[k - 1]][feature], X[order[k]][feature]
                threshold = a + (b - a) / 2
                best_cost, best = cost, (feature, a if threshold >= b else threshold)
    if best is None:
        return node
    feature, threshold = best
    left = [i for i in rows if X[i][feature] <= threshold]
    right = [i for i in rows if X[i][feature] > threshold]
    node["split"] = (feature, threshold)
    node["left"] = fit_tree(X, y, left, max_depth - 1, min_leaf, rng, max_features)
    node["right"] = fit_tree(X, y, right, max_depth - 1, min_leaf, rng, max_features)
    return node


def predict(tree, x):
    while tree["split"] is not None:
        feature, threshold = tree["split"]
        tree = tree["left"] if x[feature] <= threshold else tree["right"]
    return tree["value"]


def mse(y, predictions, rows):
    return mean([(y[i] - predictions[i]) ** 2 for i in rows])


def report(stage, y, predictions, train, validation, sampled_rows):
    """Send actual results to the native app, then optionally wait for Next."""
    event = {"kind": "stage", "stage": stage,
             "train_mse": mse(y, predictions, train),
             "validation_mse": mse(y, predictions, validation),
             "predictions": predictions, "sampled_count": len(sampled_rows),
             "sampled_unique": len(set(sampled_rows))}
    print("@LAB@" + json.dumps(event, allow_nan=False), flush=True)
    if os.environ.get("LAB_MODE") == "step":
        input()
    else:
        time.sleep(float(os.environ.get("LAB_DELAY", "0")))
    return event
