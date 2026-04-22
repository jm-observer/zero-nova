#!/usr/bin/env python3
import json
import sys
import argparse

def validate_trigger(events, skill_name):
    """
    分析 Agent 事件流，判断目标技能是否被成功触发。
    逻辑：查找 ToolStart 事件，检查其 name 或 input 中是否包含目标技能标识。
    """
    triggered = False
    details = []

    for event in events:
        if event.get("type") == "ToolStart":
            tool_name = event.get("name", "")
            tool_input = str(event.get("input", ""))
            
            # 兼容性判断逻辑：
            # 1. 直接通过工具名标识（如名为 Skill 的工具）
            # 2. 在工具输入中识别到技能路径或标识符
            if tool_name.lower() == "skill":
                triggered = True
                details.append(f"Direct match on tool 'skill': {tool_input}")
            elif skill_name in tool_input:
                triggered = True
                details.append(f"Contextual match in tool '{tool_name}': {tool_input}")

    return {
        "triggered": triggered,
        "details": details,
        "event_count": len(events)
    }

if __name__ == "__main__":
    parser = argparse.ArgumentParser(description="Validate skill trigger from nova-cli event stream")
    parser.add_argument("--events-file", required=True, help="JSON file containing events from execute_with_nova.py")
    parser.add_argument("--skill-name", required=True, help="Name of the skill to look for")
    args = parser.parse_args()

    with open(args.events_file, 'r') as f:
        events = json.load(f)
    
    result = validate_trigger(events, args.skill_name)
    print(json.dumps(result, indent=2))
