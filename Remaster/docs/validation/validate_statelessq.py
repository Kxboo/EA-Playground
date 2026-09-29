"""Corpus validation for FnStatelessQ -- thin wrapper around decoder.fn_stateless_q."""
import sys, os
_PACKAGE_ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
if _PACKAGE_ROOT not in sys.path:
    sys.path.insert(0, _PACKAGE_ROOT)

from decoder.fn_stateless_q import main

if __name__ == "__main__":
    main()
