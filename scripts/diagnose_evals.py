import json
import os
import sys
from pathlib import Path

def diagnose_eval_set(path_str):
    path = Path(path_str)
    print(f"--- Diagnosing: {path} ---")
    if not path.exists():
        print("ERROR: File does not exist!")
        return

    # Check raw bytes for BOM
    raw_bytes = path.read_bytes()
    print(f"Raw Size: {len(raw_bytes)} bytes")
    if raw_bytes.startswith(b'\xef\xbb\xbf'):
        print("Warning: UTF-8 BOM detected!")
    
    # Read text
    content = path.read_text(encoding="utf-8")
    print(f"Content length: {len(content)} chars")
    
    # Parse JSON
    try:
        data = json.loads(content)
        print(f"Top-level Type: {type(data)}")
        if isinstance(data, list):
            print(f"Items count: {len(data)}")
            if len(data) > 0:
                print(f"Example item 0 keys: {list(data[0].keys())}")
                if "should_trigger" in data[0]:
                    print(f"  - 'should_trigger' found! value: {data[0]['should_trigger']} (type: {type(data[0]['should_trigger'])})")
                else:
                    print(f"  - 'should_trigger' MISSING in item 0!")
        elif isinstance(data, dict):
            print(f"Keys found: {list(data.keys())}")
    except Exception as e:
        print(f"JSON Parse Error: {e}")

if __name__ == "__main__":
    # We'll check the last known location from target/request
    target_path = r"C:\Users\36225\AppData\Local\Temp\nova_skill_creator\tech-researcher\evals\evals.json"
    diagnose_eval_set(target_path)
