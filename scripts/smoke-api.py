"""Run against an isolated fusion-server data directory (creates test records).

Usage: python scripts/smoke-api.py http://127.0.0.1:18765
No credentials or live model calls are required.
"""
import json
import sys
import urllib.request

base = sys.argv[1].rstrip('/')
checks = 0

def call(path, body=None, method=None):
    global checks
    request = urllib.request.Request(base + path, data=None if body is None else json.dumps(body).encode(),
                                     headers={'Content-Type': 'application/json'}, method=method)
    with urllib.request.urlopen(request, timeout=60) as response:
        result = json.load(response)
    assert result['code'] == 0, (path, result.get('msg'))
    checks += 1
    return result.get('data')

project = call('/api/project', {'name': 'API smoke test'})
project_id = project['id']
try:
    overview = call(f'/api/project/{project_id}/workspace-overview')
    print('Workspace fields:', sorted(overview))
    scripts = call(f'/api/script/project/{project_id}')
    boards = call(f'/api/storyboard/project/{project_id}')
    script = scripts[0] if isinstance(scripts, list) else scripts
    board = boards[0] if isinstance(boards, list) else boards
    episode = call('/api/script/episode', {'scriptId': script['id'], 'episodeNumber': 1, 'title': 'Test episode'})
    scene = call('/api/script/scene', {'scriptId': script['id'], 'episodeId': episode['id'], 'sceneHeading': 'EXT. POND - DAY'})
    board_episode = call('/api/storyboard/episode', {'storyboardId': board['id'], 'scriptEpisodeId': episode['id'], 'title': 'Test episode'})
    board_scene = call('/api/storyboard/scene', {'storyboardId': board['id'], 'episodeId': board_episode['id'], 'sceneHeading': 'Pond'})
    item = call('/api/storyboard/item', {'storyboardId': board['id'], 'storyboardEpisodeId': board_episode['id'],
                                        'storyboardSceneId': board_scene['id'], 'content': 'A paper boat', 'duration': 6})
    assert call(f"/api/storyboard/item/{item['id']}")['content'] == 'A paper boat'
    call(f"/api/storyboard/item/{item['id']}", method='DELETE')
    call(f"/api/storyboard/scene/{board_scene['id']}", method='DELETE')
    call(f"/api/storyboard/episode/{board_episode['id']}", method='DELETE')
    call(f"/api/script/scene/{scene['id']}", method='DELETE')
    call(f"/api/script/episode/{episode['id']}", method='DELETE')
    for endpoint in ['/api/ai/model/list', '/api/asset/all', '/api/generation/image/page', '/api/generation/video/page', '/api/system/init/status']:
        call(endpoint)
finally:
    call(f'/api/project/{project_id}', method='DELETE')
print(f'PASS: {checks} API requests')
