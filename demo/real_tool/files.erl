%% The Erlang side of demo/real_tool: Tool<ReadFile>'s implementation.
%%
%% A tool handler takes the tool's argument record as a map and returns
%% the tool's result type; strings are UTF-8 binaries. It raises one of
%% the tool's declared errors by throwing `{hird_exn, Error}`, here a
%% FileError constructor as a tagged tuple.
-module(files).
-export([read/1]).

read(#{path := Path}) ->
    case file:read_file(Path) of
        {ok, Text} -> Text;
        {error, enoent} -> throw({hird_exn, {missing, Path}});
        {error, Reason} ->
            throw({hird_exn, {unreadable, atom_to_binary(Reason)}})
    end.
