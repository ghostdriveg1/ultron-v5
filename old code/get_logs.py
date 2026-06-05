import urllib.request, json
try:
    req = urllib.request.Request('http://127.0.0.1:8000/admin/logs')
    with urllib.request.urlopen(req) as response:
        logs = json.loads(response.read().decode())
        for log in logs[-20:]:
            print(f"[{log.get('timestamp')}] [{log.get('source')}] {log.get('message')}")
except Exception as e:
    print('Failed:', e)
