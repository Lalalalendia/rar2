# Consumer-only acceptance helper for current editable-target proof.
import os
import sys

import scribus


def main() -> int:
    print("chaptera-scribus-consumer: script-start", flush=True)
    output_value = os.environ.get("CHAPTERA_SCRIBUS_SLA_OUT")
    if not output_value:
        print("CHAPTERA_SCRIBUS_SLA_OUT is required", file=sys.stderr)
        return 2
    output = os.path.abspath(output_value)

    have_doc = scribus.haveDoc()
    print(f"chaptera-scribus-consumer: have-doc={have_doc}", flush=True)
    if not have_doc:
        print("scribus did not auto-open the IDML input", file=sys.stderr, flush=True)
        return 3

    print("chaptera-scribus-consumer: save-start", flush=True)
    scribus.saveDocAs(output)
    print("chaptera-scribus-consumer: save-done", flush=True)
    scribus.closeDoc()
    print("chaptera-scribus-consumer: close-done", flush=True)

    if not os.path.isfile(output) or os.path.getsize(output) == 0:
        print("scribus produced no SLA", file=sys.stderr)
        return 4

    # Scribus stays alive after a -py script unless the application is
    # explicitly told to quit. In CI that leaves docker/xvfb-run waiting even
    # though the consumer proof is already complete.
    print("chaptera-scribus-consumer: quit-start", flush=True)
    scribus.fileQuit()
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
