#!/usr/bin/env python3
"""SSE streaming demo for FusionRouter.

Usage:
    python scripts/sse-demo.py "openrouter/free" "Tell me a short joke"
    python scripts/sse-demo.py --url http://localhost:8080/v1/chat/completions
"""

import argparse
import json
import sys
import time
import urllib.request

sys.stdout.reconfigure(encoding="utf-8", errors="replace")
sys.stderr.reconfigure(encoding="utf-8", errors="replace")


def stream_chat(url: str, model: str, message: str):
    body = json.dumps({
        "model": model,
        "messages": [{"role": "user", "content": message}],
        "stream": True,
    }).encode()

    req = urllib.request.Request(url, data=body, headers={
        "Content-Type": "application/json",
        "Accept": "text/event-stream",
    })

    start = time.perf_counter()
    token_count = 0
    full_text = ""

    with urllib.request.urlopen(req, timeout=120) as resp:
        print(f"> HTTP {resp.status} {resp.reason}", file=sys.stderr)
        buffer = ""
        for chunk in iter(lambda: resp.read(1), b""):
            buffer += chunk.decode("utf-8", errors="replace")
            while "\n\n" in buffer:
                raw, buffer = buffer.split("\n\n", 1)
                for line in raw.split("\n"):
                    if line.startswith("data: "):
                        data = line[6:]
                        if data.strip() == "[DONE]":
                            break
                        try:
                            payload = json.loads(data)
                            choices = payload.get("choices", [])
                            if choices:
                                delta = choices[0].get("delta", {})
                                content = delta.get("content", "")
                                if not content:
                                    continue
                                token_count += 1
                                full_text += content
                                print(content, end="", flush=True)
                        except json.JSONDecodeError:
                            pass

    elapsed = time.perf_counter() - start

    print("\n" + "-" * 40)
    print(f"tokens={token_count}, chars={len(full_text)}, time={elapsed:.2f}s")
    return full_text


def main():
    parser = argparse.ArgumentParser(description="FusionRouter SSE streaming demo")
    parser.add_argument("model", nargs="?", default="openrouter/free",
                        help="Model ID (default: openrouter/free)")
    parser.add_argument("message", nargs="?", default="say hello in 3 words",
                        help="User message (default: 'say hello in 3 words')")
    parser.add_argument("--url", default="http://localhost:8080/v1/chat/completions",
                        help="Server URL")
    args = parser.parse_args()

    print(f"> model={args.model}")
    print(f"> url={args.url}")
    print(f"> messages=[{args.message}]")
    print("-" * 40)

    try:
        stream_chat(args.url, args.model, args.message)
    except KeyboardInterrupt:
        print("\nInterrupted.")
        sys.exit(1)
    except Exception as e:
        print(f"\nError: {e}", file=sys.stderr)
        sys.exit(1)


if __name__ == "__main__":
    main()
