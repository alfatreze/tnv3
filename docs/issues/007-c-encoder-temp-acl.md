# 007 — C adapter temporary-directory permissions

**Status:** Unix fix applied in the codec repository and host-tested. Windows
native ACL validation remains open.

The C encoder adapter previously used `create_dir` with platform defaults for
its temporary workspace. The adapter now requests mode `0700` atomically on
Unix. Its permission test checks that group and other permission bits are
clear, allowing restrictive process umasks. The Unix permission test and
Python-backed adapter integration test pass on the host.

On Windows, creation still uses the default inherited parent ACL; no explicit
ACL restriction or native Windows check has been performed. Sensitive source
media or intermediates could be exposed if the chosen temp parent has broad
access. Until a Windows gate passes, use an account-private temp parent.

**Next gate:** run a native Windows test that inspects effective ACLs and
verifies only the invoking user and intended system principals can access the
workspace. Keep Windows ACL hardening and the native test open until that gate
passes.
