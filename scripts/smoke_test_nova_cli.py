#!/usr/bin/env python3
import sys
from pathlib import Path

# Add project root to sys.path to import execute_with_nova
project_root = Path(__file__).parent.parent
sys.path.append(str(project_root))

from scripts.execute_with_nova import execute_with_nova

def main():
    print("--- Starting Nova CLI Smoke Test ---")
    
    prompt = "Hello, who are you? Just reply with one sentence."
    workspace = str(project_root / "temp_test_workspace")
    
    print(f"Testing basic prompt in {workspace}...")
    events = execute_with_nova(prompt, workspace=workspace, model="gemini-3-flash")
    
    if not events:
        print("FAILED: No events received from nova-cli")
        sys.exit(1)
        
    print(f"Received {len(events)} events:")
    for e in events:
        print(f"  {e}")
    
    # Check if we got text deltas
    text_deltas = [e for e in events if e.get("type") == "TextDelta"]
    if text_deltas:
        full_text = "".join(e["text"] for e in text_deltas)
        print(f"Agent response: {full_text}")
    else:
        print("FAILED: No TextDelta events found")
        
    # Check for TurnComplete
    complete = [e for e in events if e.get("type") == "TurnComplete"]
    if complete:
        usage = complete[0].get("usage", {})
        print(f"SUCCESS: Turn complete. Usage: {usage}")
    else:
        print("FAILED: No TurnComplete event found")

if __name__ == "__main__":
    main()
