#!/usr/bin/env python3
"""S1 error-rate curve and run-vs-run comparison (Phase 1 acceptance).

Kept as the name the Phase 1 docs use; the implementation is the scenario-
generic scripts/scenario-curve.py with --scenario s1.
"""
import runpy
import sys

sys.argv = [sys.argv[0], "--scenario", "s1", *sys.argv[1:]]
runpy.run_path(__file__.replace("s1-curve.py", "scenario-curve.py"), run_name="__main__")
