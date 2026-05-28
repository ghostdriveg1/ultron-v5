#!/bin/sh
set -e

echo "========================================================"
echo "⚡ SAOS: SUPER-AGENT OPERATING SYSTEM REDIS+WEBDIS GATEWAY"
echo "========================================================"

# 1. Create and verify the writeable data directory
mkdir -p /data
chmod 777 /data

# 2. Pull down state snapshot from Private GitHub Repository on boot
if [ -n "$GITHUB_PAT" ] && [ -n "$GITHUB_USER" ]; then
    echo "[SAOS BOOT] GitHub credentials detected. Restoring database state from private repository..."
    python3 -c "
import os, urllib.request, json, subprocess, shutil, gzip
pat = os.getenv(\"GITHUB_PAT\", \"\")
user = os.getenv(\"GITHUB_USER\", \"\")
base_repo = os.getenv(\"GITHUB_REPO\", \"saos-backups-part-1\")

try:
    req = urllib.request.Request('https://api.github.com/user/repos?sort=pushed&per_page=100')
    req.add_header('Authorization', f'token {pat}')
    req.add_header('Accept', 'application/vnd.github.v3+json')
    req.add_header('User-Agent', 'SAOS-Boot')
    
    repo_name = base_repo
    with urllib.request.urlopen(req, timeout=10) as response:
        repos = json.loads(response.read().decode())
        prefix = base_repo.split('-part-')[0] if '-part-' in base_repo else base_repo
        backup_repos = [r['name'] for r in repos if r['name'].startswith(prefix)]
        if backup_repos:
            repo_name = backup_repos[0]
            
    print(f'Found latest active backup repository: {repo_name}')
    clone_url = f'https://{user}:{pat}@github.com/{user}/{repo_name}.git'
    tmp_dir = '/tmp/saos-restore'
    
    if os.path.exists(tmp_dir):
        shutil.rmtree(tmp_dir)
        
    subprocess.run(['git', 'clone', '--depth', '1', clone_url, tmp_dir], check=True)
    
    compressed_path = os.path.join(tmp_dir, 'dump.rdb.gz')
    if os.path.exists(compressed_path):
        print('Extracting database snapshot dump.rdb.gz...')
        with gzip.open(compressed_path, 'rb') as f_in:
            with open('/data/dump.rdb', 'wb') as f_out:
                shutil.copyfileobj(f_in, f_out)
        print('Successfully restored L1 Cache from private GitHub backup repository!')
    else:
        print('No compressed dump.rdb.gz found in backup repository. Starting fresh.')
        
    if os.path.exists(tmp_dir):
        shutil.rmtree(tmp_dir)
except Exception as e:
    print(f'GitHub snapshot recovery failed (starting fresh): {e}')
"
else
    echo "[SAOS BOOT] GitHub credentials not configured. Starting fresh Redis database."
fi

# 3. Initialize background local Redis Server
echo "[SAOS BOOT] Starting background Redis database server..."
redis-server --dir /data --dbfilename dump.rdb --save 60 1 --protected-mode no &

# 4. Initialize background SAOS backup & Git-Archiver daemon
echo "[SAOS BOOT] Launching background backup & Git-Archiver sync daemon..."
python3 -u /app/backup.py > /var/log/backup.log 2>&1 &

# 5. Secure Webdis configuration and clean unexpected parameters
echo "[SAOS BOOT] Cleaning configuration and securing Webdis REST gateway..."
python3 -c "
import os, json
with open('/app/webdis.json', 'r') as f:
    data = json.load(f)
if 'workers' in data:
    del data['workers']
secret = os.getenv(\"NANCY_REDIS_SECRET\", \"\")
if secret:
    print('[SAOS BOOT] Injecting custom Basic Auth credentials into Webdis ACL.')
    for entry in data.get(\"acl\", []):
        if entry.get(\"http_profile\") == \"nancy_admin\":
            del entry[\"http_profile\"]
            entry[\"http_basic_auth\"] = f\"nancy_admin:{secret}\"
else:
    print('[WARNING] NANCY_REDIS_SECRET missing! Webdis REST server is running unsecured!')
with open('/app/webdis.json', 'w') as f:
    json.dump(data, f, indent=2)
"

# 6. Boot Webdis REST Gateway in the foreground to keep the Hugging Face container alive
echo "[SAOS BOOT] Starting Webdis HTTP REST gateway on port 7860..."
exec webdis /app/webdis.json
