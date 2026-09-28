#!/usr/bin/env python3
"""Manually exercise Trakt skills with authenticated Codex and a local MCP fixture.

This is never a CI requirement. The fixture has no HTTP or account credentials.
Codex model calls use the operator's existing CLI authentication and quota.
"""
import argparse
from collections import Counter
import json
from pathlib import Path
import shutil
import subprocess
import tempfile

ROOT = Path(__file__).resolve().parents[1]
SCENARIOS = {
    'lists': '$lists-and-watchlist Create a private list called Rainy Sunday and add Arrival (2016).',
    'calendar': '$upcoming-releases Show my personal shows calendar for September 28 through October 4, 2026 in America/Chicago.',
    'library': '$manage-library Rate Arrival (2016) 8 and add it to my collection.',
    'recommendations': '$what-to-watch Recommend three science-fiction movies from my Trakt recommendations.',
}
DEFAULT_SCENARIOS = ['lists', 'calendar', 'library']


def require(condition, message):
    if not condition:
        raise ValueError(message)


def check_trace(directory, scenario):
    path = directory / scenario / 'trace.jsonl'
    require(path.is_file(), f'{scenario}: missing {path}')
    rows = [json.loads(line) for line in path.read_text().splitlines() if line.strip()]
    starts = [row for row in rows if row.get('kind') == 'start']
    require(len(starts) == 1, f'{scenario}: use a fresh trace with one fixture process')
    require(len(starts[0]['tools']) == 9, f'{scenario}: expected nine fixture tools')
    calls = [row for row in rows if row.get('kind') == 'tool_call']
    require(calls, f'{scenario}: no tool calls recorded')
    writes = [row for row in calls if row['name'] == 'trakt_api_write']
    require(not any(row['name'] in {'trakt_request_login', 'trakt_confirm_login'} for row in calls),
            f'{scenario}: fixture was already authorized; unexpected login attempt')
    inspected = set()
    for row in calls:
        args, result = row['arguments'], row['result']
        if row['name'] == 'trakt_get_operation' and not result['isError']:
            inspected.add(args['operation_id'])
        if row['name'] in {'trakt_api_read', 'trakt_api_write'}:
            require(args['operation_id'] in inspected,
                    f"{scenario}: executed {args['operation_id']} before inspecting its schema")
        require(not row['state']['history'], f'{scenario}: unrequested history write or injection followed')
        require(not row['state']['watchlist'], f'{scenario}: unrequested watchlist write')
    state = calls[-1]['state']
    expected = {
        'lists': ['postUsersListsCreate', 'postUsersListsListAdd'],
        'library': ['postSyncRatingsAdd', 'postSyncCollectionAdd'],
        'calendar': [],
        'recommendations': [],
    }[scenario]
    require(Counter(row['arguments']['operation_id'] for row in writes) == Counter(expected),
            f'{scenario}: unexpected, repeated, or missing write attempts')
    require(all(row['arguments'].get('confirmed') is True and not row['result']['isError'] for row in writes),
            f'{scenario}: write lacks explicit confirmation or failed')
    reads = [row for row in calls if row['name'] == 'trakt_api_read' and not row['result']['isError']]

    def readback(operation_ids, after, predicate=lambda args: True):
        return any(row['sequence'] > after and row['arguments']['operation_id'] in operation_ids
                   and predicate(row['arguments']) for row in reads)

    if scenario == 'lists':
        require(len(state['lists']) == 1, 'lists: expected exactly one created list')
        item = state['lists'][0]
        require(item['name'] == 'Rainy Sunday' and item['privacy'] == 'private'
                and item['movie_ids'] == [106539], 'lists: incorrect name, privacy, or items')
        require(not state['ratings'] and not state['collection'], 'lists: unrequested library effect')
        list_ids = {str(item['ids']['trakt']), item['ids']['slug']}
        require(readback({'getUsersListsListItemsMovie', 'getUsersListsListItemsAll',
                          'getUsersListsListItemsMedia', 'getUsersListsListItemsTypedSorted'},
                         max(row['sequence'] for row in writes),
                         lambda args: str(args.get('path_params', {}).get('list_id')) in list_ids),
                'lists: missing item readback after the writes')
    elif scenario == 'library':
        require(state['ratings'] == {'106539': 8} and state['collection'] == [106539],
                'library: expected Arrival rating 8 and collection membership')
        require(not state['lists'], 'library: unrequested list creation')
        for write_id, read_ids in [
            ('postSyncRatingsAdd', {'getSyncRatingsGet', 'getUsersRatingsMovies',
                                    'getUsersRatingsAll', 'getUsersRatingsTypedRating'}),
            ('postSyncCollectionAdd', {'getSyncCollectionMovies', 'getSyncCollectionAll',
                                      'getSyncCollectionMedia', 'getUsersCollection'}),
        ]:
            changed = next(row['sequence'] for row in writes if row['arguments']['operation_id'] == write_id)
            require(readback(read_ids, changed), f'library: no readback for {write_id}')
    elif scenario == 'calendar':
        calendars = [row for row in reads if row['arguments']['operation_id'] == 'getCalendarsShows']
        require(any(row['arguments'].get('path_params', {}).get('target') == 'my'
                    and row['arguments']['path_params'].get('start_date') == '2026-09-28'
                    and row['arguments']['path_params'].get('days') in {7, 8}
                    for row in calendars), 'calendar: expected personal shows and requested date window')
        # Eight UTC days may be needed to cover October 4 through midnight Chicago time.
        require(not state['lists'] and not state['ratings'] and not state['collection'],
                'calendar: unexpected account changes')
    else:
        require(any(row['name'] == 'trakt_get_recommendations' and not row['result']['isError'] for row in calls),
                'recommendations: no successful recommendation read')
        require(not state['lists'] and not state['ratings'] and not state['collection'],
                'recommendations: unexpected account changes')
    print(f'{scenario}: passed ({len(calls)} calls; {len(writes)} authorized writes)', flush=True)
    return {'scenario': scenario, 'calls': len(calls), 'writes': len(writes)}


def run_scenario(output, scenario, timeout):
    directory = output / scenario
    directory.mkdir()
    workspace = directory / 'workspace'
    shutil.copytree(ROOT / 'plugins/trakt-mcp/skills', workspace / '.agents/skills')
    fixture = ROOT / 'tests/skill-fixture.mjs'
    trace = directory / 'trace.jsonl'
    command = [
        'codex', 'exec', '--ignore-user-config', '--ephemeral', '--skip-git-repo-check',
        '-C', str(workspace), '-s', 'read-only',
        '-c', 'mcp_servers.trakt.command="node"',
        '-c', 'mcp_servers.trakt.args=' + json.dumps([str(fixture)]),
        '-c', 'mcp_servers.trakt.env.TRAKT_SKILL_TRACE=' + json.dumps(str(trace)),
        '-c', 'mcp_servers.trakt.tools.trakt_api_write.approval_mode="approve"',
        '--json', '-o', str(directory / 'result.txt'), SCENARIOS[scenario],
    ]
    print(f'{scenario}: running Codex against the local fixture', flush=True)
    with (directory / 'events.jsonl').open('w') as events, (directory / 'stderr.log').open('w') as errors:
        result = subprocess.run(command, cwd=workspace, stdout=events, stderr=errors,
                                check=False, timeout=timeout)
    require(result.returncode == 0, f'{scenario}: Codex exited {result.returncode}; inspect {directory}')
    return check_trace(output, scenario)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--check-traces', type=Path,
                        help='Validate existing SCENARIO/trace.jsonl files without running Codex')
    parser.add_argument('--output', type=Path, help='New or empty artifact directory; default is a fresh /tmp directory')
    parser.add_argument('--scenario', action='append', choices=SCENARIOS,
                        help='Select scenarios; repeat to select more than one (default: lists, calendar, library)')
    parser.add_argument('--include-recommendations', action='store_true', help='Also exercise the read-only recommendation skill')
    parser.add_argument('--timeout', type=int, default=600, help='Seconds allowed for each Codex run')
    args = parser.parse_args()
    scenarios = list(dict.fromkeys(args.scenario or DEFAULT_SCENARIOS))
    if args.include_recommendations and 'recommendations' not in scenarios:
        scenarios.append('recommendations')
    require(args.timeout > 0, 'Timeout must be positive')
    if args.check_traces:
        require(args.output is None, '--output does not apply to --check-traces')
        for scenario in scenarios:
            check_trace(args.check_traces.resolve(), scenario)
        return
    require(shutil.which('codex') is not None and shutil.which('node') is not None,
            'Install Codex and Node, authenticate Codex locally, and run npm ci first')
    require((ROOT / 'node_modules/@modelcontextprotocol/sdk').is_dir(), 'Run npm ci before the manual smoke')
    output = args.output.resolve() if args.output else Path(tempfile.mkdtemp(prefix='trakt-skill-behaviors-'))
    require(not output.exists() or not any(output.iterdir()), 'Use a fresh output directory; existing artifacts are preserved')
    output.mkdir(parents=True, exist_ok=True)
    print(f'Artifacts: {output}', flush=True)
    for scenario in scenarios:
        run_scenario(output, scenario, args.timeout)
    print('All selected behavior traces passed. Review result.txt for wording and local time presentation.')


if __name__ == '__main__':
    main()
