#!/usr/bin/env python3
"""Generate JSON test fixtures for benchmarking jlens."""

import json
import os
import random
import string
import sys

FIXTURES_DIR = os.path.join(os.path.dirname(__file__), "fixtures")


def random_string(length=12):
    return "".join(random.choices(string.ascii_lowercase, k=length))


def random_value():
    r = random.random()
    if r < 0.3:
        return random_string(random.randint(5, 50))
    elif r < 0.5:
        return random.randint(-10000, 10000)
    elif r < 0.65:
        return random.random() * 1000
    elif r < 0.8:
        return random.choice([True, False])
    else:
        return None


def generate_object(num_keys=10):
    return {random_string(8): random_value() for _ in range(num_keys)}


def generate_array_of_objects(count, keys_per_obj=10):
    """Uniform array of objects — typical API response shape."""
    keys = [random_string(8) for _ in range(keys_per_obj)]
    items = []
    for i in range(count):
        obj = {"id": i}
        for k in keys:
            obj[k] = random_value()
        items.append(obj)
    return items


def generate_nested(depth, breadth=3):
    """Deeply nested structure."""
    if depth <= 0:
        return random_value()
    return {
        random_string(6): generate_nested(depth - 1, breadth)
        for _ in range(breadth)
    }


def generate_wide_object(num_keys):
    """Single object with many keys."""
    return {f"key_{i:06d}": random_value() for i in range(num_keys)}


def write_fixture(name, data):
    path = os.path.join(FIXTURES_DIR, name)
    with open(path, "w") as f:
        json.dump(data, f)
    size = os.path.getsize(path)
    print(f"  {name}: {size:,} bytes ({size / 1024 / 1024:.1f} MB)")


def main():
    os.makedirs(FIXTURES_DIR, exist_ok=True)
    random.seed(42)

    print("Generating fixtures...")

    # 1KB — tiny
    write_fixture("small_1kb.json", generate_array_of_objects(5, 8))

    # 100KB — typical API response
    write_fixture("medium_100kb.json", generate_array_of_objects(300, 12))

    # 1MB
    write_fixture("medium_1mb.json", generate_array_of_objects(3000, 12))

    # 10MB
    write_fixture("large_10mb.json", generate_array_of_objects(30000, 12))

    # 100MB
    write_fixture("large_100mb.json", generate_array_of_objects(300000, 12))

    # Deep nesting (10K levels would be too slow to generate as JSON, use 500)
    write_fixture("deep_500.json", generate_nested(500, 1))

    # Wide object (100K keys)
    write_fixture("wide_100k_keys.json", generate_wide_object(100000))

    print("\nDone. Fixtures in:", FIXTURES_DIR)


if __name__ == "__main__":
    main()
