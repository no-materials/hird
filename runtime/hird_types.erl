%% Copyright 2026 the Hird Authors
%% SPDX-License-Identifier: Apache-2.0 OR MIT
%%
%% Canonical wire encoding and decoding of tool-invocation records
%% (audit-log v1), type-directed against the signature table the compiler
%% emits into each base module (`hird_tools@/0`). The encoder's output
%% reproduces the golden files under conformance/v1 byte for byte, and the
%% decoder accepts exactly what the format specifies; the format lives in
%% docs/tool-effects.md.
-module(hird_types).

-export([encode_invocation/2, decode_invocation/2]).

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
-type decoded() :: #{
    tool := atom(),
    args := term(),
    result := {ok, term()} | {err, term()},
    timestamp := binary(),
    caller := binary(),
    meta => #{binary() => term()}
}.

-export_type([shape/0, table/0, record_in/0, decoded/0]).

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

%% Decoding ----------------------------------------------------------------
%%
%% The inverse of encode_invocation/2, mirroring the Rust reference decoder:
%% envelope fields in fixed order, schema version 1 only, values decoded
%% type-directedly against the tool's shapes into the runtime terms the
%% dispatcher sees (records as maps, ADT values as atoms or ctor tuples).
%% Every parser takes a binary and returns {Value, Rest} (or just Rest);
%% any flaw fails with `{decode_error, Detail}`.

%% One audit-log line decoded to runtime terms. The tool is resolved by
%% wire name through the table; the timestamp stays an RFC 3339 binary.
-spec decode_invocation(binary(), table()) -> decoded().
decode_invocation(Line, #{tools := Tools, types := Types}) ->
    R1 = dkey(<<"schema_version">>, dtok(${, Line)),
    {Version, R2} = dinteger(R1),
    Version =:= 1 orelse
        erlang:error({decode_error, {unsupported_schema_version, Version}}),
    {WireName, R3} = dstring(dkey(<<"tool">>, dtok($,, R2))),
    {Tool, #{args := AShape, result := RShape, error := EShape}} =
        tool_by_name(WireName, Tools),
    {Args, R4} = dvalue(AShape, Types, dkey(<<"args">>, dtok($,, R3))),
    {Result, R5} =
        dresult(RShape, EShape, Types, dkey(<<"result">>, dtok($,, R4))),
    {Ts, R6} = dstring(dkey(<<"timestamp">>, dtok($,, R5))),
    valid_timestamp(Ts) orelse
        erlang:error({decode_error, {bad_timestamp, Ts}}),
    {Caller, R7} = dstring(dkey(<<"caller">>, dtok($,, R6))),
    Base = #{tool => Tool, args => Args, result => Result,
             timestamp => Ts, caller => Caller},
    {Record, R8} = case ws(R7) of
        <<$,, T/binary>> ->
            {Meta, RM} = dmeta(dkey(<<"meta">>, T)),
            {Base#{meta => Meta}, RM};
        Other ->
            {Base, Other}
    end,
    case ws(dtok($}, R8)) of
        <<>> -> Record;
        _ -> erlang:error({decode_error, trailing_input})
    end.

%% The tool atom and signature for a wire name.
tool_by_name(Name, Tools) ->
    Found = [{Tool, Sig}
             || {Tool, #{name := N} = Sig} <- maps:to_list(Tools), N =:= Name],
    case Found of
        [Entry | _] -> Entry;
        [] -> erlang:error({decode_error, {unknown_tool, Name}})
    end.

%% Skips JSON whitespace.
ws(<<C, Rest/binary>>) when C =:= $\s; C =:= $\t; C =:= $\n; C =:= $\r ->
    ws(Rest);
ws(Bin) ->
    Bin.

%% Consumes exactly `Char` (after whitespace).
dtok(Char, Bin) ->
    case ws(Bin) of
        <<C, Rest/binary>> when C =:= Char -> Rest;
        _ -> erlang:error({decode_error, {expected, <<Char>>}})
    end.

%% Consumes the literal `Lit` (after whitespace).
dliteral(Lit, Bin) ->
    Size = byte_size(Lit),
    case ws(Bin) of
        <<Prefix:Size/binary, Rest/binary>> when Prefix =:= Lit -> Rest;
        _ -> erlang:error({decode_error, {expected, Lit}})
    end.

%% Consumes the object key `Name` and its `:`.
dkey(Name, Bin) ->
    {Key, Rest} = dstring(Bin),
    Key =:= Name orelse
        erlang:error({decode_error, {expected_key, Name, Key}}),
    dtok($:, Rest).

%% Parses a JSON string.
dstring(Bin) ->
    unescape(dtok($", Bin), <<>>).

unescape(<<$", Rest/binary>>, Acc) ->
    {Acc, Rest};
unescape(<<$\\, Rest/binary>>, Acc) ->
    {C, R} = descape(Rest),
    unescape(R, <<Acc/binary, C/utf8>>);
unescape(<<C, _/binary>>, _Acc) when C < 16#20 ->
    erlang:error({decode_error, raw_control_in_string});
unescape(<<C/utf8, Rest/binary>>, Acc) ->
    unescape(Rest, <<Acc/binary, C/utf8>>);
unescape(_, _Acc) ->
    erlang:error({decode_error, bad_string}).

%% Parses one string escape, the leading `\` already consumed.
descape(<<$", R/binary>>) -> {$", R};
descape(<<$\\, R/binary>>) -> {$\\, R};
descape(<<$/, R/binary>>) -> {$/, R};
descape(<<$b, R/binary>>) -> {8, R};
descape(<<$f, R/binary>>) -> {12, R};
descape(<<$n, R/binary>>) -> {$\n, R};
descape(<<$r, R/binary>>) -> {$\r, R};
descape(<<$t, R/binary>>) -> {$\t, R};
descape(<<$u, R/binary>>) -> dunicode(R);
descape(<<C, _/binary>>) -> erlang:error({decode_error, {unknown_escape, C}});
descape(<<>>) -> erlang:error({decode_error, bad_string}).

%% Parses `XXXX` of a `\uXXXX` escape, pairing surrogates.
dunicode(Bin) ->
    {High, R1} = hex4(Bin),
    if
        High >= 16#D800, High < 16#DC00 ->
            case R1 of
                <<"\\u", R2/binary>> ->
                    {Low, R3} = hex4(R2),
                    (Low >= 16#DC00) andalso (Low < 16#E000) orelse
                        erlang:error({decode_error, unpaired_surrogate}),
                    Code = 16#10000 + ((High - 16#D800) bsl 10)
                        + (Low - 16#DC00),
                    {Code, R3};
                _ ->
                    erlang:error({decode_error, unpaired_surrogate})
            end;
        High >= 16#DC00, High < 16#E000 ->
            erlang:error({decode_error, unpaired_surrogate});
        true ->
            {High, R1}
    end.

%% Parses four hex digits.
hex4(<<Hex:4/binary, Rest/binary>>) ->
    case lists:all(fun is_hex/1, binary_to_list(Hex)) of
        true -> {binary_to_integer(Hex, 16), Rest};
        false -> erlang:error({decode_error, bad_unicode_escape})
    end;
hex4(_) ->
    erlang:error({decode_error, bad_unicode_escape}).

is_hex(C) ->
    (C >= $0 andalso C =< $9)
        orelse (C >= $a andalso C =< $f)
        orelse (C >= $A andalso C =< $F).

%% The number token at the head: an optional sign, then digits and
%% [.eE+-], as the reference tokenizer spans it.
dnumber(Bin) ->
    {Sign, B1} = case ws(Bin) of
        <<$-, T/binary>> -> {<<"-">>, T};
        B -> {<<>>, B}
    end,
    {Body, Rest} = number_span(B1, <<>>),
    Body =/= <<>> orelse erlang:error({decode_error, expected_number}),
    {<<Sign/binary, Body/binary>>, Rest}.

number_span(<<C, Rest/binary>>, Acc)
        when C >= $0, C =< $9;
             C =:= $.; C =:= $e; C =:= $E; C =:= $+; C =:= $- ->
    number_span(Rest, <<Acc/binary, C>>);
number_span(Bin, Acc) ->
    {Acc, Bin}.

%% Whether a number token carries float syntax.
float_syntax(Tok) ->
    binary:match(Tok, [<<".">>, <<"e">>, <<"E">>]) =/= nomatch.

%% Parses an integer-syntax number within i64 range.
dinteger(Bin) ->
    {Tok, Rest} = dnumber(Bin),
    float_syntax(Tok) andalso
        erlang:error({decode_error, {not_an_integer, Tok}}),
    Value = try binary_to_integer(Tok)
            catch error:badarg ->
                erlang:error({decode_error, {not_an_integer, Tok}})
            end,
    (Value >= -(1 bsl 63)) andalso (Value < (1 bsl 63)) orelse
        erlang:error({decode_error, {not_an_integer, Tok}}),
    {Value, Rest}.

%% Parses a number as a float; plain integer syntax widens exactly.
dfloat(Bin) ->
    {Tok, Rest} = dnumber(Bin),
    Value = try parse_float(Tok)
            catch error:badarg ->
                erlang:error({decode_error, {not_a_number, Tok}})
            end,
    {Value, Rest}.

%% binary_to_float with the mantissa normalised to carry a decimal point
%% ("1e3" → "1.0e3"), which Erlang requires and JSON does not.
parse_float(Tok) ->
    case binary:split(Tok, [<<"e">>, <<"E">>]) of
        [Plain] ->
            case float_syntax(Plain) of
                true -> binary_to_float(Plain);
                false -> float(binary_to_integer(Plain))
            end;
        [Mant, Exp] ->
            Mant2 = case binary:match(Mant, <<".">>) of
                nomatch -> <<Mant/binary, ".0">>;
                _ -> Mant
            end,
            binary_to_float(<<Mant2/binary, "e", Exp/binary>>)
    end.

%% One value against its shape; the inverse of value/3 on its image.
dvalue(unit, _Types, Bin) ->
    {ok, dliteral(<<"null">>, Bin)};
dvalue(int, _Types, Bin) ->
    dinteger(Bin);
dvalue(float, _Types, Bin) ->
    dfloat(Bin);
dvalue(string, _Types, Bin) ->
    dstring(Bin);
dvalue(bool, _Types, Bin) ->
    {Name, R1} = dctor_open(Bin),
    Rest = dtok($}, dtok($], dtok($[, R1))),
    case Name of
        <<"True">> -> {true, Rest};
        <<"False">> -> {false, Rest};
        _ -> erlang:error({decode_error, {unknown_constructor, bool, Name}})
    end;
dvalue({list, Elem}, Types, Bin) ->
    R1 = dtok($[, Bin),
    case ws(R1) of
        <<$], Rest/binary>> -> {[], Rest};
        _ -> dlist(Elem, Types, R1, [])
    end;
dvalue({tuple, Shapes}, Types, Bin) ->
    {Values, Rest} = darray(Shapes, Types, Bin),
    {list_to_tuple(Values), Rest};
dvalue({record, Fields}, Types, Bin) ->
    R1 = dtok(${, Bin),
    {Pairs, R2} = drecord(Fields, Types, R1, true),
    {maps:from_list(Pairs), dtok($}, R2)};
dvalue({adt, Name, Params}, Types, Bin) ->
    Ctors = case maps:find(Name, Types) of
        {ok, Cs} -> Cs;
        error -> erlang:error({decode_error, {unknown_type, Name}})
    end,
    {WireName, R1} = dctor_open(Bin),
    case lists:keyfind(WireName, 2, Ctors) of
        {Tag, WireName, Shapes} ->
            Instantiated = [subst(S, Params) || S <- Shapes],
            {Values, R2} = darray(Instantiated, Types, R1),
            Value = case Values of
                [] -> Tag;
                _ -> list_to_tuple([Tag | Values])
            end,
            {Value, dtok($}, R2)};
        false ->
            erlang:error({decode_error, {unknown_constructor, Name, WireName}})
    end;
dvalue(Shape, _Types, _Bin) ->
    erlang:error({decode_error, {undecodable, Shape}}).

%% `{"ctor":Name,"args":` consumed, the wire constructor name returned.
dctor_open(Bin) ->
    {Name, R1} = dstring(dkey(<<"ctor">>, dtok(${, Bin))),
    {Name, dkey(<<"args">>, dtok($,, R1))}.

%% Comma-separated values of one element shape, up to `]`.
dlist(Elem, Types, Bin, Acc) ->
    {V, R1} = dvalue(Elem, Types, Bin),
    case ws(R1) of
        <<$,, R2/binary>> -> dlist(Elem, Types, R2, [V | Acc]);
        R2 -> {lists:reverse([V | Acc]), dtok($], R2)}
    end.

%% A fixed-arity array of shape-directed values.
darray(Shapes, Types, Bin) ->
    {Values, R1} = delems(Shapes, Types, dtok($[, Bin), true),
    {Values, dtok($], R1)}.

delems([], _Types, Bin, _First) ->
    {[], Bin};
delems([Shape | Shapes], Types, Bin, First) ->
    B1 = case First of true -> Bin; false -> dtok($,, Bin) end,
    {V, R1} = dvalue(Shape, Types, B1),
    {Vs, R2} = delems(Shapes, Types, R1, false),
    {[V | Vs], R2}.

%% Record fields in declared (sorted) order, keys exact.
drecord([], _Types, Bin, _First) ->
    {[], Bin};
drecord([{Label, Shape} | Fields], Types, Bin, First) ->
    B1 = case First of true -> Bin; false -> dtok($,, Bin) end,
    B2 = dkey(atom_to_binary(Label, utf8), B1),
    {V, R1} = dvalue(Shape, Types, B2),
    {Vs, R2} = drecord(Fields, Types, R1, false),
    {[{Label, V} | Vs], R2}.

%% The tagged result field: `{"ok":…}` against the result shape,
%% `{"err":…}` against the error shape.
dresult(RShape, EShape, Types, Bin) ->
    {Tag, R1} = dstring(dtok(${, Bin)),
    R2 = dtok($:, R1),
    {Result, R3} = case Tag of
        <<"ok">> ->
            {OkV, OkR} = dvalue(RShape, Types, R2),
            {{ok, OkV}, OkR};
        <<"err">> ->
            {ErrV, ErrR} = dvalue(EShape, Types, R2),
            {{err, ErrV}, ErrR};
        _ ->
            erlang:error({decode_error, {bad_result_tag, Tag}})
    end,
    {Result, dtok($}, R3)}.

%% A self-describing meta object: binary keys, plain terms.
dmeta(Bin) ->
    R1 = dtok(${, Bin),
    case ws(R1) of
        <<$}, Rest/binary>> -> {#{}, Rest};
        _ -> dmeta_pairs(R1, #{})
    end.

dmeta_pairs(Bin, Acc) ->
    {Key, R1} = dstring(Bin),
    {Value, R2} = dmeta_value(dtok($:, R1)),
    case ws(R2) of
        <<$,, R3/binary>> -> dmeta_pairs(R3, Acc#{Key => Value});
        R3 -> {Acc#{Key => Value}, dtok($}, R3)}
    end.

%% One self-describing meta value.
dmeta_value(Bin) ->
    case ws(Bin) of
        <<"null", R/binary>> -> {null, R};
        <<"true", R/binary>> -> {true, R};
        <<"false", R/binary>> -> {false, R};
        <<$", _/binary>> = B -> dstring(B);
        <<${, _/binary>> = B -> dmeta(B);
        <<$[, R/binary>> ->
            case ws(R) of
                <<$], R2/binary>> -> {[], R2};
                _ -> dmeta_list(R, [])
            end;
        B ->
            {Tok, R} = dnumber(B),
            Value = try
                case float_syntax(Tok) of
                    true -> parse_float(Tok);
                    false -> binary_to_integer(Tok)
                end
            catch error:badarg ->
                erlang:error({decode_error, {not_a_number, Tok}})
            end,
            {Value, R}
    end.

dmeta_list(Bin, Acc) ->
    {V, R1} = dmeta_value(Bin),
    case ws(R1) of
        <<$,, R2/binary>> -> dmeta_list(R2, [V | Acc]);
        R2 -> {lists:reverse([V | Acc]), dtok($], R2)}
    end.

%% `YYYY-MM-DDTHH:MM:SS.mmmZ` with in-range fields (days checked against
%% 31, not the month's calendar length).
valid_timestamp(<<Y:4/binary, $-, Mo:2/binary, $-, D:2/binary, $T,
                  H:2/binary, $:, Mi:2/binary, $:, S:2/binary, $.,
                  Ms:3/binary, $Z>>) ->
    case lists:all(fun all_digits/1, [Y, Mo, D, H, Mi, S, Ms]) of
        true ->
            [MoV, DV, HV, MiV, SV] =
                [binary_to_integer(B) || B <- [Mo, D, H, Mi, S]],
            MoV >= 1 andalso MoV =< 12 andalso DV >= 1 andalso DV =< 31
                andalso HV < 24 andalso MiV < 60 andalso SV < 60;
        false ->
            false
    end;
valid_timestamp(_) ->
    false.

all_digits(Bin) ->
    lists:all(fun(C) -> C >= $0 andalso C =< $9 end, binary_to_list(Bin)).
