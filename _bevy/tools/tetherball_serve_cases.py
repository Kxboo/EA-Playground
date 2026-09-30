"""Targeted inputs only; expectations must come from the original PPC oracle."""
import copy
import struct


def extend_cases(cases, bits):
    """Append independent cases and return the same list (no expected values)."""
    seed = copy.deepcopy(cases[1])

    def case(label, human=True):
        c = copy.deepcopy(seed)
        for key in ('expected', 'expected_ball', 'expected_aux', 'effects', 'returned'):
            c.pop(key, None)
        c['label'] = 'target-' + label
        s, a = c['initial'], c['aux']
        s.update(paused=False, server=0, receiver=1, focus_player=0,
                 session_mode=2, round_timer_ms=10000, action_states=[2, 2],
                 serve_bubble_visible=True, latches=[False, True], postgame_win_flag=False)
        s['match_state'].update(state_code=27, state_ms=0, elapsed_ms=100,
                                match_over=False, rotations=[0, 0], round_wins=[0, 0])
        for p in range(2):
            s['players'][p].update(controller=p if human else None, current_animation=58)
        a.update(word_224=-1, counter_260=0, counter_264=0, counter_270=99,
                 pause_block_count_0fc=0, pause_menu_open=False, forced_ai=[False, False],
                 field_25c=False, field_229=True, power_serve_enabled=False,
                 return_angles=[bits(1.), bits(2.)])
        # focus_player==0 requests the complement interval: 1.5 is outside.
        c['ball'].update(angle=bits(1.5), tossed=True, height=bits(1.2), vertical_velocity=bits(0.))
        c['input']['events'] = {}
        c['milliseconds'] = 16
        cases.append(c)
        return c

    # Human high-predicate gate/type/mega priority, including forced AI routing.
    for kind in (-1, 5, 6, 7):
        for event in (False, True):
            for enabled in (False, True):
                c = case(f'human-type-{kind}-event-{event}-enabled-{enabled}')
                c['aux'].update(word_224=kind, power_serve_enabled=enabled)
                c['input']['events'] = {'0': {'92': event}}
    for forced in (False, True):
        for anim in (58, 60, 91, 93):
            for action in (0, 1, 2):
                c = case(f'route-forced-{forced}-anim-{anim}-action-{action}')
                c['aux']['forced_ai'][0] = forced
                c['initial']['players'][0]['current_animation'] = anim
                c['initial']['action_states'] = [action, 1]
                c['ball']['tossed'] = False
                c['initial']['match_state']['state_ms'] = 2801

    for state_ms in (1999, 2000, 2001, 2799, 2800, 2801, 0x7fffffff, 0x80000000, 0xffffffff):
        for height, velocity in ((1.2, 0.53), (1.2, 2.), (0.1, -1.)):
            c = case(f'ai-edge-{state_ms}-height-{height}-velocity-{velocity}', False)
            c['initial']['match_state']['state_ms'] = state_ms
            c['ball'].update(tossed=True, height=bits(height), vertical_velocity=bits(velocity))

    for human in (False, True):
        c = case(f'toss-visible-bubble-human-{human}', human)
        c['ball']['tossed'] = False
        c['initial']['action_states'][0] = 1
        c['initial']['match_state']['state_ms'] = 2001
    c = case('human-mega-global-enabled')
    c['initial']['postgame_win_flag'] = True
    c['input']['events'] = {'0': {'92': True}}

    # Neighboring f32 input bits around analytical candidates. This does not
    # calculate predicates or expected outputs; PPC decides every result.
    seconds = 0.11
    power_centers = (4.807 * seconds - 0.5, 4.807 * seconds + 0.5)
    high_center = 0.7 + 2.4035 * seconds * seconds
    for mode, centers in (('power', power_centers), ('high', (high_center,))):
        for center_i, center in enumerate(centers):
            base = bits(center)
            for adjacent in range(-4, 5):
                for human in (False, True):
                    c = case(f'{mode}-threshold-{center_i}-bits-{adjacent}-human-{human}', human)
                    c['initial']['match_state']['state_ms'] = 2801
                    if mode == 'power':
                        c['ball']['vertical_velocity'] = base + adjacent
                    else:
                        c['ball'].update(height=base + adjacent, vertical_velocity=bits(0.))

    for timer in (0, 1, 16, 17, 0x7fffffff, 0x80000000, 0xffffffff):
        for dt in (-1, 0, 16, 0x7fffffff, -0x80000000):
            c = case(f'signed-counter-{timer}-delta-{dt}')
            c['initial']['round_timer_ms'] = struct.unpack('>i', struct.pack('>I', timer))[0]
            c['initial']['latches'][0] = True
            c['aux'].update(counter_260=timer, counter_264=timer, word_224=5)
            c['milliseconds'] = dt

    for focus in (0, 1):
        for angle in (0.99999994, 1., 1.00000012, 1.5, 1.99999988, 2., 2.00000024, -4.7831853, 7.7831853):
            for winner in (-1, 0, 1):
                c = case(f'cross-focus-{focus}-angle-{angle}-winner-{winner}')
                c['initial'].update(focus_player=focus)
                c['initial']['players'][0]['current_animation'] = 60
                c['ball']['angle'] = bits(angle)
                if winner >= 0:
                    c['initial']['match_state']['rotations'][winner] = 6
                    c['initial']['match_state']['round_wins'][winner] = 1
                c['rules'].update(mode=0, rotation_limit=6, wins_required=2, time_limit_seconds=30000)

    for block in (-1, 0, 1):
        for already_open in (False, True):
            c = case(f'pause-two-events-block-{block}-open-{already_open}')
            c['aux'].update(pause_block_count_0fc=block, pause_menu_open=already_open)
            c['input']['events'] = {'0': {'175': True}, '1': {'175': True}}
    return cases
