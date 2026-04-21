import json
from pathlib import Path

def test_json_load_logic(eval_set_path):
    print(f"Testing JSON from: {eval_set_path}")
    content = Path(eval_set_path).read_text(encoding="utf-8")
    data = json.loads(content)
    
    # 模拟 run_loop.py 的核心逻辑
    print(f"Parsed type: {type(data)}")
    
    if isinstance(data, dict):
        print("DETECTED ISSUE: JSON is a DICTIONARY, but run_loop expects a LIST!")
        if "evals" in data:
            print("Found 'evals' key. Re-wrapping as list...")
            actual_list = data["evals"]
            print(f"Success! Extracted {len(actual_list)} items.")
            return actual_list
        else:
            print("CRITICAL: No 'evals' key found in the dictionary.")
            return None
    elif isinstance(data, list):
        print("SUCCESS: JSON is already a LIST as expected.")
        return data

if __name__ == "__main__":
    target = r"C:\Users\36225\AppData\Local\Temp\nova_skill_creator\tech-solution-architect\evals\evals.json"
    result = test_json_load_logic(target)
    
    if result:
        # 如果需要修复，直接在这里执行局部修复
        Path(target).write_text(json.dumps(result, indent=2), encoding="utf-8")
        print(f"File {target} has been REPAIRED to a pure LIST format.")
