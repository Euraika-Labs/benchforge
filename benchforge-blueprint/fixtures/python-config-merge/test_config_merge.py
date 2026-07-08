from copy import deepcopy
import unittest

from config_merge import merge_config


class MergeConfigTests(unittest.TestCase):
    def test_merges_nested_dicts_without_dropping_siblings(self):
        base = {
            "model": {
                "temperature": 0,
                "limits": {"max_tokens": 512, "timeout_seconds": 30},
            },
            "features": ["json"],
            "enabled": True,
        }
        override = {
            "model": {"limits": {"max_tokens": 1024}},
            "features": ["json", "streaming"],
        }

        result = merge_config(base, override)

        self.assertEqual(
            result,
            {
                "model": {
                    "temperature": 0,
                    "limits": {"max_tokens": 1024, "timeout_seconds": 30},
                },
                "features": ["json", "streaming"],
                "enabled": True,
            },
        )

    def test_does_not_mutate_inputs(self):
        base = {"nested": {"a": 1}, "keep": True}
        override = {"nested": {"b": 2}}
        base_before = deepcopy(base)
        override_before = deepcopy(override)

        result = merge_config(base, override)

        self.assertEqual(result, {"nested": {"a": 1, "b": 2}, "keep": True})
        self.assertEqual(base, base_before)
        self.assertEqual(override, override_before)

    def test_non_dict_override_replaces_value(self):
        self.assertEqual(
            merge_config({"retry": {"count": 2}}, {"retry": False}),
            {"retry": False},
        )


if __name__ == "__main__":
    unittest.main()
