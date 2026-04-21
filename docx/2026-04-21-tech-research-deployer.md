# Design Document: Tech Research & Automated Deployment Skill

**Date:** 2026-04-21
**Project Status:** Design Phase
**Author:** Nova (via skill-creator)

## 1. Goal
Create a high-intelligence skill that automates the entire lifecycle of technical research and deployment: from initial concept searching and GitHub-based deep dives to proposing a structured open-source solution, environment-specific adaptation, and final automated deployment/testing.

## 2. Detailed Design

### 2.1 Workflow Stages
1.  **Phase 1: Discovery (Search)**
    *   **Action:** Use `web_search` to identify core technologies and trending open-source projects.
    *   **Constraint:** Must prioritize projects with active GitHub repositories and permissive licenses.
2.  **Phase 2: Extraction (GitHub/Docs)**
    *   **Action:** Use `web_fetch` to ingest `README.md`, `INSTALL.md`, or official documentation from identified GitHub repos.
    *   **Output:** A technical summary of the tool's capabilities and requirements.
3.  **Phase 3: Proposal (User Interaction)**
    *   **Format:** A structured Markdown report including:
        *   Technology Overview
        *   Recommended Open Source Stack
        *   Prerequisites (OS, Dependencies)
        *   Step-by-step Deployment Plan
    *   **Gate:** **STOP** and wait for user approval.
4.  **Phase 4: Contextual Adaptation (Environment Research)**
    *   **Action:** Upon approval, search for specific implementation nuances in the user's target environment (e.g., "deploying [Project] on Ubuntu 22.04 with Docker").
5.  **Phase 5: Execution (Deploy & Test)**
    *   **Action:** Generate and run deployment scripts (bash/powershell/docker-compose).
    *   **Verification:** Run automated tests (e.g., checking service status, port availability, or API responsiveness) to ensure the deployment succeeded.

### 2.2 Interface & Data Flow
*   **Input:** User's technical query/interest.
*   **Internal State:** Maintains context of the researched technology and the chosen version/repository.
*   **Output:** 
    *   Intermediate: Structured Research Report.
    *   Final: Deployment status and verification results.

### 2.3 Test Case Strategy
To ensure the skill is robust, the following test types will be used:
*   **Case A (Simple):** A well-documented tool (e.g., "How to deploy Redis via Docker").
*   **Case B (Complex):** A multi-component stack (e.g., "Setup a Prometheus/Grafana monitoring stack").
*   **Case C (Edge Case):** A tool requiring specific OS configurations (e.g., "Install a specific database on a restricted Linux environment").

## 3. Risks & Mitigations
| Risk | Mitigation |
| :--- | :--- |
| **Hallucination of commands** | Strictly mandate the use of `web_fetch` on official docs before generating any deployment scripts. |
| **Security/Malware** | Use only verified GitHub stars/trends and include a "Security Review" step in the proposal. |
| **Environment mismatch** | Force a "Pre-flight Check" stage to verify OS/Dependencies before attempting deployment. |
| **Looping/Failed Deployments** | Implement a maximum retry limit and a fallback to "Manual Intervention Required" mode. |

## 4. Implementation Plan
1.  Create `SKILL.md` in a temporary directory.
2.  Define detailed `instructions` emphasizing the "Search -> GitHub -> Propose -> Confirm" loop.
3.  Develop the `evals.json` with the three test cases mentioned above.
4.  Run the `run_loop.py` in `--non-interactive` mode to optimize the triggering description and the workflow's reliability.
