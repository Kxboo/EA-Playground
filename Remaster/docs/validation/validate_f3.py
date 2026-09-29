"""Corpus validation for FnDeltaF3 -- thin wrapper around decoder.fn_delta_f3."""
import sys, os
_PACKAGE_ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
if _PACKAGE_ROOT not in sys.path:
    sys.path.insert(0, _PACKAGE_ROOT)

from decoder.fn_delta_f3 import main

if __name__ == "__main__":
    main()
