# Design Document: Tech Solution Architect Skill

**Date:** 2026-04-21
**Project Status:** Draft
**Goal:** Create a skill that automates technical research, solution proposal, and environment-specific deployment/testing for open-source technologies.

## 1. Overview
The `tech-solution-architect` skill is designed to transition a user from a vague technical interest ("How do I use Redis?") to a fully deployed and tested instance ("Redis is running on port 6379 with successful ping").

## 2. Detailed Design

### 2.1 Workflow Stages
The skill will operate in a stateful manner, progressing through the following phases:

| Phase | Action | Tools Used | Success Criteria |
| :--- | :--- | :--- | :--- |
| **1. Research** | Search for tech terms, compare open-source options. | `web_search` | A list of 2-3 viable open-source candidates with pros/cons. |
| **2. Deep Dive** | Scrape GitHub/Docs for installation/config details. | `web_fetch` | Extraction of exact installation commands and dependency lists. |
| **3. Proposal** | Generate a structured Markdown proposal. | Internal reasoning | User approval of the selected candidate and deployment plan. |
| **4. Contextualize**| Search for specific environment nuances (e.g., ARM64, Docker, WSL). | `web_search` | Identification of potential pitfalls for the target environment. |
| **5. Deploy & Test**| Execute commands and verify status. | `bash` | Command exit code 0 AND functional verification (e.g., `curl` or `ping`). |

### 2.2 Technical Architecture
- **State Management**: The skill will rely on the conversational context to track which phase it is in.
- **Information Extraction**: Use specialized prompts to transform raw web text into structured JSON/Markdown for the "Proposal" phase.
- **Deployment Engine**: A series of `bash` calls. To ensure safety and repeatability, the skill will prioritize `docker` or `conda/venv` based environments where possible.

### 2.3 Test Cases (For Unattended Optimization)
To enable the `run_loop.py` optimization, we will use the following test profiles:

| Test ID | Topic | Expected Outcome | Verification Method |
| :--- | :--- | :--- | :--- |
| **TC-01** | Lightweight KV Store (e.g., Redis) | Successful install & `redis-cli ping` | Check process + CLI response |
| **TC-02** | Static Web Server (e.g., Nginx) | Running on port 80/8080 | `curl -I localhost:80` |
| **TC-03** | Python-based Tool (e.g., HTTP Server) | Functional Python script execution | Verify `stdout` content |

### 2.4 Risks & Mitigations
- **Risk**: Web search returns non-open-source or paid solutions.
  - *Mitigation*: Explicitly add "open-source" and "free" to search queries and include a "license check" step in the research phase.
- **Risk**: Deployment commands fail due to environment differences.
  - *Mitigation*: Phase 4 (Contextualize) is mandatory to bridge the gap between "generic docs" and "user's actual environment".
- **Risk**: Infinite loops in research.
  - *Mitigation*: Set a hard limit on the number of search queries per phase.

## 3. Implementation Plan
1. Create the temporary workspace in OS temp directory.
2. Write `SKILL.md` with a "pushy" description to ensure it triggers on technical queries.
3. Implement the `scripts/` for any repetitive parsing if needed.
4. Run the `run_loop.py` with the defined Evals.
