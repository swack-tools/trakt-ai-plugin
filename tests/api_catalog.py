"""Offline regressions for catalog normalization and real request contracts."""
import importlib.util
import json
from pathlib import Path
import unittest

from jsonschema import Draft202012Validator, FormatChecker

ROOT = Path(__file__).resolve().parents[1]
SPEC = importlib.util.spec_from_file_location('update_trakt_catalog', ROOT / 'scripts/update_trakt_catalog.py')
GENERATOR = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(GENERATOR)
CATALOG = {operation['operation_id']: operation for operation in json.loads((ROOT / 'api/trakt/catalog.json').read_text())['operations']}


class CatalogTests(unittest.TestCase):
    def assert_valid(self, schema, value):
        errors = list(Draft202012Validator(schema, format_checker=FormatChecker()).iter_errors(value))
        self.assertEqual([], errors, '\n'.join(error.message for error in errors))

    def assert_invalid(self, schema, value):
        self.assertFalse(Draft202012Validator(schema, format_checker=FormatChecker()).is_valid(value))

    def body(self, operation):
        return CATALOG[operation]['request_body']['schema']

    def test_generated_schemas_are_valid(self):
        for operation in CATALOG.values():
            for parameter in operation['parameters']:
                Draft202012Validator.check_schema(parameter['schema'])
            if operation['request_body'] is not None:
                Draft202012Validator.check_schema(operation['request_body']['schema'])

    def test_catalog_integrity(self):
        GENERATOR.offline_check()

    def test_comment_composition_accepts_shared_fields(self):
        schema = self.body('postCommentsPost')
        valid = {'comment':'This is a sufficiently long fixture comment.', 'spoiler':False, 'movie':{'ids':{'trakt':112}}}
        self.assert_valid(schema, valid)
        self.assert_invalid(schema, {**valid, 'unexpected':True})
        self.assert_invalid(schema, {**valid, 'movie':None})
        self.assert_invalid(schema, {**valid, 'show':{'ids':{'trakt':2}}})
        self.assert_invalid(schema, {'comment':'A sufficiently long fixture comment.', 'spoiler':False})
        self.assert_invalid(schema, {**valid, 'movie':{'ids':{'trakt':112}, 'unexpected':True}})

    def test_history_accepts_multiple_identifiers(self):
        schema = self.body('postSyncHistoryAdd')
        self.assert_valid(schema, {'movies':[{'ids':{'trakt':112, 'imdb':'tt2543164'}, 'watched_at':'2026-09-27T18:00:00Z'}]})
        self.assert_invalid(schema, {'movies':[{'ids':{}}]})
        self.assert_invalid(schema, {'movies':[{'ids':{'trakt':112, 'unknown':1}}]})

    def test_list_create_keeps_description_and_closes_unknown_fields(self):
        schema = self.body('postUsersListsCreate')
        self.assert_valid(schema, {'name':'Rainy Sunday', 'description':'Fixture list', 'privacy':'private'})
        self.assert_invalid(schema, {'name':'Rainy Sunday', 'unexpected':True})

    def test_list_item_variants_accept_show_ids_without_duplicate_branch_failure(self):
        for operation in ('postUsersListsListAdd', 'postUsersListsListRemove'):
            schema = self.body(operation)
            self.assert_valid(schema, {'shows':[{'ids':{'trakt':456}}]})
            self.assert_invalid(schema, {'shows':[{'ids':{}}]})
            self.assert_invalid(schema, {'shows':[{'ids':{'trakt':456}, 'unexpected':True}]})
        source = {'oneOf':[
            {'type':'object','required':['ids'],'properties':{'ids':{'type':'object'}}},
            {'type':'object','required':['ids'],'properties':{'ids':{'type':'object'},'seasons':{'type':'array'}}},
        ]}
        normalized = GENERATOR.json_schema(source)
        self.assertIn('anyOf', normalized)
        self.assert_valid(normalized, {'ids':{}})

    def test_calendar_range_and_date(self):
        parameters = {p['name']:p['schema'] for p in CATALOG['getCalendarsShows']['parameters'] if p['in']=='path'}
        self.assert_valid(parameters['days'], 33)
        for value in [0, -1, 34]:
            self.assert_invalid(parameters['days'], value)
        self.assert_valid(parameters['start_date'], '2026-09-28')
        for value in ['tomorrow', '2026-02-30', '2026-9-28']:
            self.assert_invalid(parameters['start_date'], value)

    def test_allof_nested_objects_close_at_correct_scope(self):
        source = {'allOf':[{'type':'object', 'properties':{'a':{'type':'string'}}}, {'type':'object', 'properties':{'b':{'type':'object', 'properties':{'c':{'type':'integer'}}}}}]}
        schema = GENERATOR.json_schema(source)
        self.assert_valid(schema, {'a':'ok', 'b':{'c':2}})
        self.assert_invalid(schema, {'a':'ok', 'b':{'c':2, 'extra':True}})
        self.assert_invalid(schema, {'a':'ok', 'b':{'c':2}, 'extra':True})

    def test_explicit_free_form_objects_remain_open(self):
        schema = GENERATOR.json_schema({'type':'object', 'properties':{'name':{'type':'string'}}, 'additionalProperties':True})
        self.assert_valid(schema, {'name':'ok', 'extra':True})


if __name__ == '__main__':
    unittest.main()
