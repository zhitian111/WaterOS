#!/bin/sh
# SPDX-License-Identifier: GPL-2.0-or-later
# Copyright (c) Linux Test Project, 2016-2023
# Copyright (c) 2015-2018 Oracle and/or its affiliates. All Rights Reserved.
# Copyright (c) International Business Machines  Corp., 2001

VERSION=${VERSION:=4.2}
NFILES=${NFILES:=1000}
SOCKET_TYPE="${SOCKET_TYPE:-tcp}"
NFS_TYPE=${NFS_TYPE:=nfs}

nfs_usage()
{
	echo "-t x    Socket type, tcp or udp, default is udp"
	echo "-v x    NFS version, default is '3'"
}

nfs_parse_args()
{
	case "$1" in
	v) VERSION="$(echo $2 | tr ',' ' ')";;
	t) SOCKET_TYPE="$(echo $2 | tr ',' ' ')";;
	*) [ "$NFS_PARSE_ARGS_CALLER" ] && $NFS_PARSE_ARGS_CALLER "$@";;
	esac
}

NFS_PARSE_ARGS_CALLER="$TST_PARSE_ARGS"
TST_OPTS="v:t:$TST_OPTS"
TST_PARSE_ARGS=nfs_parse_args
TST_USAGE=nfs_usage
TST_ALL_FILESYSTEMS=1
TST_SKIP_FILESYSTEMS="exfat,ext2,ext3,fuse,ntfs,vfat,tmpfs"
TST_MOUNT_DEVICE=1
TST_FORMAT_DEVICE=1
TST_NEEDS_ROOT=1
TST_NEEDS_CMDS="$TST_NEEDS_CMDS mount exportfs mount.nfs"
TST_SETUP="${TST_SETUP:-nfs_setup}"
TST_CLEANUP="${TST_CLEANUP:-nfs_cleanup}"
TST_NEEDS_DRIVERS="nfsd"

# When set and test is using netns ($TST_USE_NETNS set) NFS traffic will go
# through lo interface instead of ltp_ns_veth* netns interfaces (useful for
# debugging whether test failures are related to veth/netns).
LTP_NFS_NETNS_USE_LO=${LTP_NFS_NETNS_USE_LO:-}

get_socket_type()
{
	local t
	local k=0
	for t in $SOCKET_TYPE; do
		if [ "$k" -eq "$1" ]; then
			echo "${t}${TST_IPV6}"
			return
		fi
		k=$(( k + 1 ))
	done
}

# directory mounted by NFS client
get_local_dir()
{
	local v="$1"
	local n="$2"

	echo "$TST_TMPDIR/$v/$n"
}

# directory on NFS server
get_remote_dir()
{
	local v="$1"
	local n="$2"

	echo "$TST_MNTPOINT/$v/$n"
}

nfs_get_remote_path()
{
	local v
	local type=$(get_socket_type ${2:-0})

	for v in $VERSION; do
		break;
	done

	v=${1:-$v}
	echo "$(get_remote_dir $v $type)"
}

nfs_server_udp_enabled()
{
	local config f

	tst_rhost_run -c "[ -f /etc/nfs.conf ]" || return 0
	config=$(tst_rhost_run -c 'for f in $(grep ^include.*= '/etc/nfs.conf' | cut -d = -f2); do [ -f $f ] && printf "$f "; done')

	tst_rhost_run -c "grep -q \"^[# ]*udp *= *y\" /etc/nfs.conf $config"
}

nfs_setup_server()
{
	local remote_dir="$1"
	local fsid="$2"
	local export_cmd="exportfs -i -o fsid=$fsid,no_root_squash,rw *:$remote_dir"

	[ -z "$fsid" ] && tst_brk TBROK "empty fsid"

	if tst_net_use_netns; then
		if ! test -d $remote_dir; then
			mkdir -p $remote_dir; $export_cmd
		fi
	else
		if ! tst_rhost_run -c "test -d $remote_dir"; then
			tst_rhost_run -s -c "mkdir -p $remote_dir; $export_cmd"
		fi
	fi
}

nfs_mount()
{
	local local_dir="$1"
	local remote_dir="$2"
	local opts="$3"
	local host_type=rhost
	local mount_dir

	mkdir -p "$local_dir"

	tst_net_use_netns && host_type=

	if [ $TST_IPV6 ]; then
		mount_dir="[$(tst_ipaddr $host_type)]:$remote_dir"
	else
		mount_dir="$(tst_ipaddr $host_type):$remote_dir"
	fi

	local mnt_cmd="mount -v -t nfs $opts $mount_dir $local_dir"

	tst_res TINFO "Mounting NFS: $mnt_cmd"
	if tst_net_use_netns && [ -z "$LTP_NFS_NETNS_USE_LO" ]; then
		tst_rhost_run -c "$mnt_cmd" > mount.log
	else
		$mnt_cmd > mount.log
	fi

	if [ $? -ne 0 ]; then
		cat mount.log

		if [ "$type" = "udp" -o "$type" = "udp6" ] && tst_kvcmp -ge 5.6; then
			tst_brk TCONF "UDP support disabled with the kernel config NFS_DISABLE_UDP_SUPPORT?"
		fi

		if grep -iq "Protocol not supported" mount.log; then
			tst_brk TCONF "Protocol not supported"
		fi

		tst_brk TBROK "mount command failed"
	fi

	cd "$local_dir"
}

nfs_setup()
{
	local i
	local type
	local n=0
	local local_dir
	local remote_dir
	local mount_dir

	if [ "$(stat -f . | grep "Type: nfs")" ]; then
		tst_brk TCONF "Cannot run nfs-stress test on mounted NFS"
	fi

	if tst_cmd_available pgrep; then
		for i in rpc.mountd rpc.statd; do
			pgrep $i > /dev/null || tst_brk TCONF "$i not running"
		done
	fi

	tst_res TINFO "$(mount.nfs -V)"

	for i in $VERSION; do
		type=$(get_socket_type $n)
		tst_res TINFO "setup NFSv$i, socket type $type"

		if [ "$type" = "udp" -o "$type" = "udp6" ] && ! nfs_server_udp_enabled; then
			tst_brk TCONF "UDP support disabled on NFS server"
		fi

		remote_dir="$(get_remote_dir $i $type)"
		nfs_setup_server "$remote_dir" "$(($$ + n))"
		local_dir="$(get_local_dir $i $n)"
		tst_res TINFO "Mounting $local_dir"
		nfs_mount "$local_dir" "$remote_dir" "-o proto=$type,vers=$i"

		n=$(( n + 1 ))
	done
}

nfs_cleanup()
{
	tst_res TINFO "Cleaning up testcase"
	cd $LTPROOT

	local i
	local type
	local local_dir
	local remote_dir

	local n=0
	for i in $VERSION; do
		local_dir="$(get_local_dir $i $n)"
		if grep -q "$local_dir" /proc/mounts; then
			tst_res TINFO "Unmounting $local_dir"
			umount $local_dir || tst_res TWARN "Unmount failed"
		fi
		n=$(( n + 1 ))
	done
	sleep 2

	n=0
	for i in $VERSION; do
		type=$(get_socket_type $n)
		remote_dir="$(get_remote_dir $i $type)"

		if tst_net_use_netns; then
			if test -d $remote_dir; then
				exportfs -u *:$remote_dir
				sleep 1
				rm -rf $remote_dir
			fi
		else
			tst_rhost_run -c "test -d $remote_dir && exportfs -u *:$remote_dir"
			sleep 1
			tst_rhost_run -c "test -d $remote_dir && rm -rf $remote_dir"
		fi

		n=$(( n + 1 ))
	done
}

. tst_net.sh
