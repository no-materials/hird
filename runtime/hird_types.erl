%% Copyright 2026 the Hird Authors
%% SPDX-License-Identifier: Apache-2.0 OR MIT
%%
%% Canonical wire encoding of tool-invocation records (audit-log v1),
%% type-directed against the signature table the compiler emits into each
%% base module (`hird_tools@/0`). The output reproduces the golden files
%% under conformance/v1 byte for byte; the format is specified in
%% docs/tool-effects.md.
-module(hird_types).

-export([encode_invocation/2]).

-type shape() ::
    unit | int | float | string | bool
    | {list, shape()} | {tuple, [shape()]}
    | {record, [{atom(), shape()}]}
    | {adt, atom(), [shape()]}
    | {param, non_neg_integer()}
    | dynamic.
-type table() :: #{
    tools := #{atom() := #{name := binary(),
                           args := shape(),
                           result := shape(),
                           error := shape()}},
    types := #{atom() := [{atom(), binary(), [shape()]}]}
}.
-type record_in() :: #{
    tool := atom(),
    args := term(),
    result := {ok, term()} | {err, term()},
    timestamp := integer(),
    caller := binary(),
    meta => #{atom() | binary() => term()}
}.

-export_type([shape/0, table/0, record_in/0]).

%% One record as a canonical JSON line (no trailing newline): envelope
%% fields in fixed order, no whitespace, sorted record labels and meta keys.
%% The timestamp is a millisecond system time. Fails with
%% {unknown_tool, Tool} for a tool absent from the table and
%% {unencodable, Shape, Value} on any shape/value mismatch.
-spec encode_invocation(record_in(), table()) -> binary().
encode_invocation(Record, #{tools := Tools, types := Types}) ->
    #{tool := Tool, args := Args, result := Result,
      timestamp := Ts, caller := Caller} = Record,
    is_map_key(Tool, Tools) orelse erlang:error({unknown_tool, Tool}),
    #{name := Name, args := AShape, result := RShape, error := EShape} =
        maps:get(Tool, Tools),
    Tagged = case Result of
        {ok, Value} -> [<<"{\"ok\":">>, value(RShape, Types, Value), $}];
        {err, Value} -> [<<"{\"err\":">>, value(EShape, Types, Value), $}]
    end,
    Meta = case maps:find(meta, Record) of
        {ok, M} -> [<<",\"meta\":">>, meta(M)];
        error -> []
    end,
    iolist_to_binary([
        <<"{\"schema_version\":1,\"tool\":">>, string(Name),
        <<",\"args\":">>, value(AShape, Types, Args),
        <<",\"result\":">>, Tagged,
        <<",\"timestamp\":">>, string(timestamp(Ts)),
        <<",\"caller\":">>, string(Caller),
        Meta, $}
    ]).

%% One value against its shape.
-spec value(shape(), map(), term()) -> iodata().
value(unit, _Types, ok) ->
    <<"null">>;
value(int, _Types, I) when is_integer(I) ->
    integer_to_binary(I);
value(float, _Types, F) when is_float(F) ->
    float_bin(F);
value(string, _Types, S) when is_binary(S) ->
    string(S);
value(bool, _Types, true) ->
    <<"{\"ctor\":\"True\",\"args\":[]}">>;
value(bool, _Types, false) ->
    <<"{\"ctor\":\"False\",\"args\":[]}">>;
value({list, Elem}, Types, L) when is_list(L) ->
    [$[, join([value(Elem, Types, V) || V <- L]), $]];
value({tuple, Shapes}, Types, T)
        when is_tuple(T), tuple_size(T) =:= length(Shapes) ->
    [$[, join(zip(Shapes, tuple_to_list(T), Types)), $]];
value({record, Fields}, Types, Map) when is_map(Map) ->
    Encoded = [[string(atom_to_binary(Label, utf8)), $:,
                value(Shape, Types, maps:get(Label, Map))]
               || {Label, Shape} <- Fields],
    [${, join(Encoded), $}];
value({adt, Name, Params}, Types, V) ->
    Ctors = maps:get(Name, Types),
    {Tag, Fields} = case V of
        Atom when is_atom(Atom) -> {Atom, []};
        Tuple when is_tuple(Tuple), tuple_size(Tuple) > 1 ->
            [T | Fs] = tuple_to_list(Tuple),
            {T, Fs}
    end,
    case lists:keyfind(Tag, 1, Ctors) of
        {Tag, WireName, Shapes} when length(Shapes) =:= length(Fields) ->
            Instantiated = [subst(S, Params) || S <- Shapes],
            [<<"{\"ctor\":">>, string(WireName), <<",\"args\":[">>,
             join(zip(Instantiated, Fields, Types)), <<"]}">>];
        _ ->
            erlang:error({unencodable, {adt, Name, Params}, V})
    end;
value(Shape, _Types, V) ->
    erlang:error({unencodable, Shape, V}).

%% Field values against their shapes, in order.
zip([], [], _Types) -> [];
zip([S | Ss], [V | Vs], Types) -> [value(S, Types, V) | zip(Ss, Vs, Types)].

%% A constructor field shape with the ADT's parameters instantiated.
subst({param, N}, Params) -> lists:nth(N + 1, Params);
subst({list, S}, Params) -> {list, subst(S, Params)};
subst({tuple, Ss}, Params) -> {tuple, [subst(S, Params) || S <- Ss]};
subst({record, Fs}, Params) ->
    {record, [{L, subst(S, Params)} || {L, S} <- Fs]};
subst({adt, Name, Args}, Params) ->
    {adt, Name, [subst(A, Params) || A <- Args]};
subst(Shape, _Params) -> Shape.

%% A millisecond system time as RFC 3339 UTC with millisecond precision.
timestamp(Ms) when is_integer(Ms) ->
    Opts = [{unit, millisecond}, {offset, "Z"}],
    list_to_binary(calendar:system_time_to_rfc3339(Ms, Opts)).

%% A JSON string: `"` and `\` escaped, control characters as their short
%% escapes or \u00XX, everything else (including non-ASCII bytes) verbatim.
string(Bin) when is_binary(Bin) ->
    [$", escape(Bin), $"].

escape(<<>>) -> [];
escape(<<$", Rest/binary>>) -> [<<"\\\"">> | escape(Rest)];
escape(<<$\\, Rest/binary>>) -> [<<"\\\\">> | escape(Rest)];
escape(<<8, Rest/binary>>) -> [<<"\\b">> | escape(Rest)];
escape(<<12, Rest/binary>>) -> [<<"\\f">> | escape(Rest)];
escape(<<$\n, Rest/binary>>) -> [<<"\\n">> | escape(Rest)];
escape(<<$\r, Rest/binary>>) -> [<<"\\r">> | escape(Rest)];
escape(<<$\t, Rest/binary>>) -> [<<"\\t">> | escape(Rest)];
escape(<<B, Rest/binary>>) when B < 16#20 ->
    [io_lib:format("\\u~4.16.0b", [B]) | escape(Rest)];
escape(<<B, Rest/binary>>) -> [B | escape(Rest)].

%% A float in its shortest round-trip decimal form, plain notation; integral
%% values print without a fraction (1, not 1.0). BEAM floats are always
%% finite, so the spec's non-finite rejection has no case to hit here.
float_bin(F) ->
    case binary:split(float_to_binary(F, [short]), <<"e">>) of
        [Plain] -> strip_dot_zero(Plain);
        [Mant, Exp] -> expand(Mant, binary_to_integer(Exp))
    end.

strip_dot_zero(Bin) ->
    Size = byte_size(Bin) - 2,
    case Bin of <<Head:Size/binary, ".0">> -> Head; _ -> Bin end.

%% Scientific notation to plain: shift the decimal point by the exponent.
expand(Mant, Exp) ->
    {Sign, Unsigned} = case Mant of
        <<"-", R/binary>> -> {<<"-">>, R};
        _ -> {<<>>, Mant}
    end,
    [Int, Frac0] = binary:split(Unsigned, <<".">>),
    Frac = string:trim(Frac0, trailing, "0"),
    Digits = <<Int/binary, Frac/binary>>,
    Point = byte_size(Int) + Exp,
    Width = byte_size(Digits),
    if
        Point >= Width ->
            [Sign, Digits, binary:copy(<<"0">>, Point - Width)];
        Point > 0 ->
            [Sign, binary:part(Digits, 0, Point), $.,
             binary:part(Digits, Point, Width - Point)];
        true ->
            [Sign, <<"0.">>, binary:copy(<<"0">>, -Point), Digits]
    end.

%% Observer-populated meta: self-describing JSON, keys sorted.
meta(Map) when is_map(Map) ->
    Fields = lists:sort([{key(K), V} || {K, V} <- maps:to_list(Map)]),
    [${, join([[string(K), $:, meta_value(V)] || {K, V} <- Fields]), $}].

key(K) when is_atom(K) -> atom_to_binary(K, utf8);
key(K) when is_binary(K) -> K.

meta_value(null) -> <<"null">>;
meta_value(true) -> <<"true">>;
meta_value(false) -> <<"false">>;
meta_value(I) when is_integer(I) -> integer_to_binary(I);
meta_value(F) when is_float(F) -> float_bin(F);
meta_value(S) when is_binary(S) -> string(S);
meta_value(L) when is_list(L) -> [$[, join([meta_value(V) || V <- L]), $]];
meta_value(M) when is_map(M) -> meta(M).

%% Comma-joins already-encoded items.
join(Items) -> lists:join($,, Items).
