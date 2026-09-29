"""
paths.py
----------
Central path resolution for the EAGL ANM pipeline. All decoder/exporter/
validation modules resolve data file locations through here instead of
hardcoding relative paths like "player_anims.anm" -- that only worked
when scripts were run from one specific working directory. This module
resolves everything relative to the PACKAGE ROOT (the directory containing
this repo's data/ folder), so the pipeline runs correctly regardless of
the caller's current working directory.

Override via environment variables if your data lives elsewhere:
  EAGL_ANM_PATH, EAGL_SKE_PATH, EAGL_ELF_PATH, EAGL_DATA_DIR
"""

import os

_THIS_DIR = os.path.dirname(os.path.abspath(__file__))
PACKAGE_ROOT = os.path.dirname(_THIS_DIR)  # decoder/ -> package root

DATA_DIR = os.environ.get("EAGL_DATA_DIR", os.path.join(PACKAGE_ROOT, "data"))

ANM_PATH = os.environ.get("EAGL_ANM_PATH", os.path.join(DATA_DIR, "player_anims.anm"))
SKE_PATH = os.environ.get("EAGL_SKE_PATH", os.path.join(DATA_DIR, "player_skel.ske"))
ELF_PATH = os.environ.get("EAGL_ELF_PATH", os.path.join(DATA_DIR, "playgroundz.elf"))

OUTPUT_DIR = os.environ.get("EAGL_OUTPUT_DIR", os.path.join(PACKAGE_ROOT, "output", "anm_export"))
