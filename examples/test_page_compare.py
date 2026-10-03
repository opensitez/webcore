import copy
import unittest
from page_compare import compare_snapshots


def snapshots():
    style = dict(display='Block',position='Static',float='None',box_sizing='ContentBox',
        direction='LTR',writing_mode='HorizontalTb',text_align='Start',font_family='Arial',
        flex_direction='Row',flex_wrap='Nowrap',align_items='Stretch',justify_content='FlexStart',
        overflow=['Visible','Visible'],visibility='true',opacity=1.0,font_size=16,
        font_weight='Value(400)',resolved_padding=[0]*4,resolved_margin=[0]*4,
        node_id=42,id='card',tag='div',**{'class':'card','box':{'border':[10,20,100,50]}})
    chrome_style = dict(style,display='block',position='static',float='none',box_sizing='content-box',
        visibility='visible',font_size='16px',font_weight='400')
    colors = {'color':[0,0,0,255],'background':[0,0,0,0]}
    path = 'html:nth-of-type(1) > body:nth-of-type(1) > div:nth-of-type(1)'
    web = {'url':'http://localhost/test','loading':False,'elements':[
        {'path':path,'computed':style,'colors':colors,'image':{}}]}
    chrome = {'url':web['url'],'ready':'complete','fonts':'loaded','viewport':{},'elements':[
        {'path':path,'id':'card','tag':'div','class':'card','computed':chrome_style,
         'colors':colors,'rect':[10,20,100,50],'has_box':True,'transformed':False,'image':None}]}
    return web,chrome


class ComparisonTests(unittest.TestCase):
    def test_identical(self):
        report = compare_snapshots(*snapshots())
        self.assertEqual(report['summary']['matched'],1)
        self.assertEqual(report['differences'],[])

    def test_geometry_and_style_are_reported(self):
        web,chrome = snapshots()
        web['elements'][0]['computed']['font_size'] = 24
        chrome['elements'][0]['rect'][2] = 200
        report = compare_snapshots(web,chrome)
        changes = report['differences'][0]['changes']
        self.assertEqual({x['property'] for x in changes},{'font_size','geometry.width'})
        self.assertEqual(report['differences'][0]['node_id'],42)

    def test_tolerance_and_transforms(self):
        web,chrome = snapshots()
        chrome['elements'][0]['rect'][0] += 0.5
        self.assertFalse(compare_snapshots(web,chrome)['differences'])
        self.assertTrue(compare_snapshots(web,chrome,0.1)['differences'])
        chrome['elements'][0]['transformed'] = True
        self.assertFalse(compare_snapshots(web,chrome,0.1)['differences'])

    def test_unique_id_matches_moved_node(self):
        web,chrome = snapshots()
        chrome['elements'][0]['path'] = 'html:nth-of-type(1) > div:nth-of-type(2)'
        self.assertEqual(compare_snapshots(web,chrome)['summary']['matched'],1)

    def test_mismatched_identity_is_not_geometry_error(self):
        web,chrome = snapshots()
        chrome['elements'][0]['id'] = 'other'
        report = compare_snapshots(web,chrome)
        self.assertEqual(report['summary']['matched'],0)
        self.assertEqual({d['kind'] for d in report['differences']}, {'unmatched-webcore','unmatched-chrome'})

    def test_duplicate_ids_do_not_fallback(self):
        web,chrome = snapshots()
        chrome['elements'].append(copy.deepcopy(chrome['elements'][0]))
        for i,e in enumerate(chrome['elements']): e['path'] = f'div:nth-of-type({i+2})'
        self.assertEqual(compare_snapshots(web,chrome)['summary']['matched'],0)

    def test_loading_and_viewport_warnings(self):
        web,chrome = snapshots()
        web['loading'] = True
        web['viewport'] = {'width':800}
        chrome['viewport'] = {'width':1200}
        self.assertEqual(len(compare_snapshots(web,chrome)['warnings']),2)

    def test_unique_ancestor_anchor_survives_parser_path_difference(self):
        web,chrome = snapshots()
        web['elements'][0]['computed']['id'] = ''
        chrome['elements'][0]['id'] = ''
        web['elements'][0]['anchor'] = chrome['elements'][0]['anchor'] = '2|tqdiv:nth-of-type(1)'
        web['elements'][0]['path'] = 'html:nth-of-type(1) > body:nth-of-type(2) > div:nth-of-type(1)'
        self.assertEqual(compare_snapshots(web,chrome)['summary']['matched'],1)

    def test_shared_parent_displacement_is_annotated(self):
        web,chrome = snapshots()
        parent_path = web['elements'][0]['path']
        for snap in (web,chrome):
            child = copy.deepcopy(snap['elements'][0])
            child['path'] += ' > div:nth-of-type(1)'
            if snap is web:
                child['computed']['id'] = 'child'
                child['computed']['node_id'] = 43
            else: child['id'] = 'child'
            snap['elements'].append(child)
        for e in web['elements']: e['computed']['box']['border'][1] += 30
        report = compare_snapshots(web,chrome)
        self.assertEqual(report['differences'][1]['changes'][0]['same_offset_as_parent'],parent_path)
        self.assertEqual(report['summary']['elements_with_independent_differences'],1)

    def test_inserted_sibling_does_not_mismatch_following_content(self):
        web,chrome = snapshots()
        w,c = web['elements'][0],chrome['elements'][0]
        w['computed']['id'] = c['id'] = ''
        w['text'] = c['text'] = 'The same article'
        c['path'] = c['path'].replace('div:nth-of-type(1)','div:nth-of-type(2)')
        extra = copy.deepcopy(c)
        extra['path'] = w['path']; extra['text'] = 'An inserted article'
        chrome['elements'].insert(0,extra)
        result = compare_snapshots(web,chrome)
        self.assertEqual(result['summary']['matched'],1)
        self.assertEqual(result['summary']['match_methods'],{'unique-content':1})
        self.assertEqual(result['differences'][0]['element']['text'],'An inserted article')

    def test_missing_subtree_is_grouped_without_discarding_children(self):
        web,chrome = snapshots()
        web['elements'] = []
        child = copy.deepcopy(chrome['elements'][0])
        child['path'] += ' > img:nth-of-type(1)'
        child['tag'] = 'img'; child['id'] = ''
        chrome['elements'].append(child)
        result = compare_snapshots(web,chrome)
        self.assertEqual(len(result['differences']),2)
        self.assertEqual(len(result['triage']),1)
        self.assertEqual(result['triage'][0]['unmatched_descendants'],1)

    def test_hidden_element_is_not_reported_as_missing(self):
        web,chrome = snapshots()
        web['elements'][0]['hidden_by'] = 'display:none on parent'
        result = compare_snapshots(web,chrome)
        self.assertEqual(result['differences'][0]['kind'],'visibility-mismatch')
        self.assertEqual(result['summary']['matched'],1)

    def test_class_token_order_does_not_change_identity(self):
        web,chrome = snapshots()
        web['elements'][0]['computed']['id'] = chrome['elements'][0]['id'] = ''
        web['elements'][0]['computed']['class'] = 'card selected'
        chrome['elements'][0]['class'] = 'selected card'
        self.assertFalse(compare_snapshots(web,chrome)['differences'])

    def test_text_change_on_identified_element(self):
        web,chrome = snapshots()
        web['elements'][0]['text'] = 'Missing words'
        chrome['elements'][0]['text'] = 'Missing words are here'
        self.assertEqual(compare_snapshots(web,chrome)['differences'][0]['changes'][0]['property'],'text')

    def test_unique_container_and_descendants_survive_parent_path_shift(self):
        web,chrome = snapshots()
        web['elements'][0]['computed']['id'] = chrome['elements'][0]['id'] = ''
        chrome['elements'][0]['path'] = chrome['elements'][0]['path'].replace('body:nth-of-type(1)','body:nth-of-type(2)')
        for snap in (web,chrome):
            child = copy.deepcopy(snap['elements'][0]); child['path'] += ' > span:nth-of-type(1)'
            node = child['computed'] if snap is web else child
            node['tag'] = 'span'; node['class'] = ''
            snap['elements'].append(child)
        report = compare_snapshots(web,chrome)
        self.assertEqual(report['summary']['matched'],2)
        self.assertEqual(report['summary']['match_methods'],{'unique-class-signature':1,'matched-parent-path':1})

    def test_collapse_reports_coverage_gap_not_328_false_visibility_errors(self):
        web,chrome = snapshots()
        web['elements'][0]['computed']['visibility'] = 'false'
        chrome['elements'][0]['computed']['visibility'] = 'collapse'
        report = compare_snapshots(web,chrome)
        self.assertFalse(report['differences'])
        self.assertIn('collapse-specific',report['warnings'][0])


if __name__ == '__main__':
    unittest.main()
