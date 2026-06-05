#!/usr/bin/env python3
"""
deploy_spaces.py — Push all Space files to Hugging Face via huggingface_hub.
Run this in the Antigravity cloud terminal.
"""

import os
from huggingface_hub import HfApi

import os
HF_TOKEN = os.getenv("HF_TOKEN")
ORG = "ultron-v5"

api = HfApi(token=HF_TOKEN)

BASE = r"C:\Users\LOQ\nancy"
MEMORY_BASE = os.path.join(BASE, "spaces", "memory-base")

# 1. Upload memory shard base to all 4 spaces
memory_spaces = ["olympus-memory-l1", "olympus-memory-l3", "olympus-memory-l4", "olympus-memory-rnd"]

for space in memory_spaces:
    repo_id = f"{ORG}/{space}"
    print(f"\n📦 Uploading to {repo_id}...")
    api.upload_folder(
        folder_path=MEMORY_BASE,
        repo_id=repo_id,
        repo_type="space"
    )

# 2. Upload gateway files
gateway_repo = f"{ORG}/olympus-gateway"
print(f"\n📦 Uploading to {gateway_repo}...")

# Upload specific files/folders
gateway_files = [
    ("Dockerfile", "Dockerfile"),
    ("Cargo.toml", "Cargo.toml"),
    ("gateway", "gateway"),
    ("crates", "crates"),
    ("dashboard", "dashboard"),
    ("gateway/README.md", "README.md"),
]

for local_path, hf_path in gateway_files:
    full_path = os.path.join(BASE, local_path)
    if os.path.isdir(full_path):
        api.upload_folder(
            folder_path=full_path,
            repo_id=gateway_repo,
            repo_type="space",
            path_in_repo=hf_path
        )
    else:
        api.upload_file(
            path_or_fileobj=full_path,
            path_in_repo=hf_path,
            repo_id=gateway_repo,
            repo_type="space"
        )

print("\n🚀 All files uploaded via huggingface_hub! Spaces will now begin building.")
