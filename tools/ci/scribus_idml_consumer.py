# Consumer-only acceptance helper for current editable-target proof.
import os
import sys

import scribus


def main() -> int:
    output_value = os.environ.get("CHAPTERA_SCRIBUS_SLA_OUT")
    if not output_value:
        print("CHAPTERA_SCRIBUS_SLA_OUT is required", file=sys.stderr)
        return 2
    output = os.path.abspath(output_value)

    if not scribus.haveDoc():
        print("scribus did not auto-open the IDML input", file=sys.stderr)
        return 3

    scribus.saveDocAs(output)
    scribus.closeDoc()

    if not os.path.isfile(output) or os.path.getsize(output) == 0:
        print("scribus produced no SLA", file=sys.stderr)
        return 4

    # Scribus stays alive after a -py script unless the application is
    # explicitly told to quit. In CI that leaves docker/xvfb-run waiting even
    # though the consumer proof is already complete.
    scribus.fileQuit()
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
