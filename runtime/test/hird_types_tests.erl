%% Copyright 2026 the Hird Authors
%% SPDX-License-Identifier: Apache-2.0 OR MIT
%%
%% Encoder and decoder conformance: the goldens under conformance/v1
%% reproduced byte for byte and round-tripped through the decoder, plus the
%% canonical-form corners (floats, escapes, shapes) the goldens do not
%% reach.
-module(hird_types_tests).

-include_lib("eunit/include/eunit.hrl").

%% The demo tools' signature table, as codegen emits it.
table() ->
    #{tools => #{
          read_repo => #{
              name => <<"ReadRepo">>,
              args => {record, [{path, string}]},
              result => {record, [{files, {list, string}}, {status, string}]},
              error => dynamic},
          create_ticket => #{
              name => <<"CreateTicket">>,
              args => {record, [{body, string}, {title, string}]},
              result => {adt, ticket_id, []},
              error => dynamic},
          http_get => #{
              name => <<"HttpGet">>,
              args => {record, [{url, string}]},
              result => {record, [{status, int}]},
              error => {adt, http_error, []}}},
      types => #{
          ticket_id => [{ticket_id, <<"TicketId">>, [string]}],
          http_error => [{http_error, <<"HttpError">>, [int, string]}]}}.

ms(Rfc3339) ->
    calendar:rfc3339_to_system_time(Rfc3339, [{unit, millisecond}]).

%% The three golden records, as the runtime would assemble them.
records() ->
    [{"read_repo_ok.json",
      #{tool => read_repo,
        args => #{path => <<"/home/user/repo">>},
        result => {ok, #{files => [], status => <<"clean">>}},
        timestamp => ms("2026-05-22T12:00:00.000Z"),
        caller => <<"Planner.plan_repo">>,
        meta => #{duration_ms => 42}}},
     {"create_ticket_ok.json",
      #{tool => create_ticket,
        args => #{body => <<"Investigate flaky CI on main">>,
                  title => <<"Flaky CI">>},
        result => {ok, {ticket_id, <<"TCK-42">>}},
        timestamp => ms("2026-05-22T12:00:01.250Z"),
        caller => <<"Planner.plan_repo">>}},
     {"http_get_err.json",
      #{tool => http_get,
        args => #{url => <<"https://ci.example/status">>},
        result => {err, {http_error, 503, <<"service unavailable">>}},
        timestamp => ms("2026-05-22T12:00:02.000Z"),
        caller => <<"Planner.check_ci">>,
        meta => #{duration_ms => 1200}}}].

golden(Name) ->
    {ok, Bytes} = file:read_file(filename:join("../conformance/v1", Name)),
    Bytes.

reproduces_golden_files_test() ->
    lists:foreach(
        fun({Name, Record}) ->
            Line = hird_types:encode_invocation(Record, table()),
            ?assertEqual(golden(Name), <<Line/binary, $\n>>)
        end,
        records()).

reproduces_golden_log_test() ->
    Lines = [hird_types:encode_invocation(R, table()) || {_, R} <- records()],
    Log = iolist_to_binary([[L, $\n] || L <- Lines]),
    ?assertEqual(golden("planner_log.jsonl"), Log).

encode(Shape, Value) ->
    encode(Shape, Value, #{}).

encode(Shape, Value, Types) ->
    Table = #{tools => #{t => #{name => <<"T">>, args => Shape,
                                result => unit, error => dynamic}},
              types => Types},
    Line = hird_types:encode_invocation(
        #{tool => t, args => Value, result => {ok, ok},
          timestamp => 0, caller => <<"M.f">>}, Table),
    [_, Rest] = binary:split(Line, <<"\"args\":">>),
    [Args, _] = binary:split(Rest, <<",\"result\"">>),
    Args.

floats_are_shortest_round_trip_plain_notation_test_() ->
    [?_assertEqual(Expected, encode(float, Value))
     || {Expected, Value} <- [
            {<<"3.14">>, 3.14},
            {<<"1">>, 1.0},
            {<<"-0">>, -0.0},
            {<<"100000000000000000000">>, 1.0e20},
            {<<"10000000000000000">>, 1.0e16},
            {<<"0.0000001">>, 1.0e-7},
            {<<"0.000000125">>, 1.25e-7},
            {<<"-0.0025">>, -0.0025},
            {<<"0.30000000000000004">>, 0.1 + 0.2}]].

strings_escape_quotes_backslashes_and_controls_test() ->
    ?assertEqual(
        <<"\"a\\\"b\\\\c\\n\\t\\u0001\x{c3}\x{a9}\"">>,
        encode(string, <<"a\"b\\c\n\t\x01é"/utf8>>)).

unit_bool_list_tuple_test_() ->
    [?_assertEqual(<<"null">>, encode(unit, ok)),
     ?_assertEqual(<<"{\"ctor\":\"True\",\"args\":[]}">>, encode(bool, true)),
     ?_assertEqual(<<"[1,2]">>, encode({list, int}, [1, 2])),
     ?_assertEqual(<<"[1,\"x\"]">>,
                   encode({tuple, [int, string]}, {1, <<"x">>}))].

generic_adt_instantiates_parameters_test() ->
    Types = #{option => [{some, <<"Some">>, [{param, 0}]},
                         {none, <<"None">>, []}]},
    ?assertEqual(
        <<"{\"ctor\":\"Some\",\"args\":[[7]]}">>,
        encode({adt, option, [{list, int}]}, {some, [7]}, Types)),
    ?assertEqual(
        <<"{\"ctor\":\"None\",\"args\":[]}">>,
        encode({adt, option, [int]}, none, Types)).

unknown_tool_is_an_error_test() ->
    ?assertError({unknown_tool, ghost},
                 hird_types:encode_invocation(
                     #{tool => ghost, args => ok, result => {ok, ok},
                       timestamp => 0, caller => <<"M.f">>},
                     #{tools => #{}, types => #{}})).

dynamic_shape_is_an_error_test() ->
    ?assertError({unencodable, dynamic, 1}, encode(dynamic, 1)).

%% Decoding ----------------------------------------------------------------

%% Every golden line decodes to the record the runtime assembled, and
%% re-encoding the decoded record reproduces the golden bytes.
decode_round_trips_golden_files_test() ->
    lists:foreach(
        fun({Name, Record}) ->
            [Line, <<>>] = binary:split(golden(Name), <<"\n">>),
            Decoded = hird_types:decode_invocation(Line, table()),
            #{tool := Tool, args := Args, result := Result,
              timestamp := Ts, caller := Caller} = Decoded,
            ?assertEqual(maps:get(tool, Record), Tool),
            ?assertEqual(maps:get(args, Record), Args),
            ?assertEqual(maps:get(result, Record), Result),
            ?assertEqual(maps:get(caller, Record), Caller),
            Reencoded = hird_types:encode_invocation(
                Decoded#{timestamp := ms(binary_to_list(Ts))}, table()),
            ?assertEqual(golden(Name), <<Reencoded/binary, $\n>>)
        end,
        records()).

decode(Shape, Args) ->
    decode(Shape, Args, #{}).

decode(Shape, Args, Types) ->
    Table = #{tools => #{t => #{name => <<"T">>, args => Shape,
                                result => unit, error => dynamic}},
              types => Types},
    Line = <<"{\"schema_version\":1,\"tool\":\"T\",\"args\":", Args/binary,
             ",\"result\":{\"ok\":null},"
             "\"timestamp\":\"2026-05-22T12:00:00.000Z\","
             "\"caller\":\"M.f\"}">>,
    maps:get(args, hird_types:decode_invocation(Line, Table)).

decode_value_corners_test_() ->
    [?_assertEqual(ok, decode(unit, <<"null">>)),
     ?_assertEqual(true, decode(bool, <<"{\"ctor\":\"True\",\"args\":[]}">>)),
     ?_assertEqual([1, 2], decode({list, int}, <<"[1,2]">>)),
     ?_assertEqual({1, <<"x">>}, decode({tuple, [int, string]},
                                        <<"[1,\"x\"]">>)),
     ?_assertEqual(1.0, decode(float, <<"1">>)),
     ?_assertEqual(1.0e20, decode(float, <<"100000000000000000000">>)),
     ?_assertEqual(0.1 + 0.2, decode(float, <<"0.30000000000000004">>)),
     ?_assertEqual(<<"a\"b\\c\n\t\x01é"/utf8>>,
                   decode(string,
                          <<"\"a\\\"b\\\\c\\n\\t\\u0001\x{c3}\x{a9}\"">>)),
     ?_assertError({decode_error, {not_an_integer, <<"1.0">>}},
                   decode(int, <<"1.0">>)),
     ?_assertError({decode_error, {undecodable, dynamic}},
                   decode(dynamic, <<"1">>))].

decode_generic_adt_instantiates_parameters_test() ->
    Types = #{option => [{some, <<"Some">>, [{param, 0}]},
                         {none, <<"None">>, []}]},
    ?assertEqual({some, [7]},
                 decode({adt, option, [{list, int}]},
                        <<"{\"ctor\":\"Some\",\"args\":[[7]]}">>, Types)),
    ?assertEqual(none,
                 decode({adt, option, [int]},
                        <<"{\"ctor\":\"None\",\"args\":[]}">>, Types)).

%% One canonical line to tamper with in the rejection tests.
canonical_line() ->
    [Line, <<>>] = binary:split(golden("http_get_err.json"), <<"\n">>),
    Line.

decode_rejects_malformed_lines_test_() ->
    Line = canonical_line(),
    Swap = fun(From, To) -> binary:replace(Line, From, To) end,
    [?_assertError({decode_error, {unsupported_schema_version, 2}},
                   hird_types:decode_invocation(
                       Swap(<<"\"schema_version\":1">>,
                            <<"\"schema_version\":2">>), table())),
     ?_assertError({decode_error, {unknown_tool, <<"Ghost">>}},
                   hird_types:decode_invocation(
                       Swap(<<"\"HttpGet\"">>, <<"\"Ghost\"">>), table())),
     ?_assertError({decode_error, {unknown_constructor, http_error,
                                   <<"HttpErr">>}},
                   hird_types:decode_invocation(
                       Swap(<<"\"HttpError\"">>, <<"\"HttpErr\"">>), table())),
     ?_assertError({decode_error, {expected_key, <<"url">>, <<"uri">>}},
                   hird_types:decode_invocation(
                       Swap(<<"\"url\"">>, <<"\"uri\"">>), table())),
     ?_assertError({decode_error, {bad_timestamp, <<"2026-13-22T12:00:02.000Z">>}},
                   hird_types:decode_invocation(
                       Swap(<<"2026-05-22">>, <<"2026-13-22">>), table())),
     ?_assertError({decode_error, trailing_input},
                   hird_types:decode_invocation(
                       <<Line/binary, "x">>, table())),
     ?_assertError({decode_error, _},
                   hird_types:decode_invocation(<<"not json">>, table()))].

%% The decoder enforces the envelope's fixed field order.
decode_rejects_reordered_envelope_test() ->
    Line = <<"{\"tool\":\"HttpGet\",\"schema_version\":1}">>,
    ?assertError({decode_error, {expected_key, <<"schema_version">>,
                                 <<"tool">>}},
                 hird_types:decode_invocation(Line, table())).
