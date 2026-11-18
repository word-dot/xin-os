# echo -e "\n=============== Build RootFS ===============\n"
# 
# /musl/busybox mkdir -v /bin
# /musl/busybox ln -v -s /musl/busybox /bin/ln
# ln -v -s /musl/busybox /bin/mkdir
# ln -v -s /musl/busybox /bin/less
# ln -v -s /musl/busybox /bin/cp
# ln -v -s /musl/busybox /bin/rm
# /musl/busybox --install -s /bin
# export PATH=/bin
# 
# mkdir -v /lib
# mkdir -v /usr
# mkdir -v -p /var/tmp
# 
# cp /glibc/lib/libc.so.6 /lib/libc.so.6
# ln -v -s /glibc/lib/libm.so.6 /lib/libm.so.6
# ln -v -s /lib/libc.so.6 /lib/libc.so
# ln -v -s /lib/libm.so.6 /lib/libm.so
# if [[ $ARCH == loongarch64 ]]; then
#   ln -v -s /musl/lib/libc.so /lib/ld-musl-loongarch-lp64d.so.1
#   ln -v -s /glibc/lib/ld-linux-loongarch-lp64d.so.1 /lib/ld-linux-loongarch-lp64d.so.1
# else
#   ln -v -s /musl/lib/libc.so /lib/ld-musl-riscv64.so.1
#   ln -v -s /musl/lib/libc.so /lib/ld-musl-riscv64-sf.so.1
#   ln -v -s /glibc/lib/ld-linux-riscv64-lp64d.so.1 /lib/ld-linux-riscv64-lp64d.so.1
# fi
# ln -v -s /lib /lib64
# ln -v -s /lib /usr/lib
# ln -v -s /lib /usr/lib64
# 
# export LD_LIBRARY_PATH=".:./lib:/musl/lib:/lib"
# echo -e "\n=============== Final Round Stage 2 ===============\n"
# 
# cd /musl
# export HOME=/musl
# cp -r ./usr/share /usr
# cp ./usr/bin/git /bin
# cp ./usr/bin/git-daemon /bin
# ./gitnetwork_testcode.sh
# 
# cd /glibc
# export HOME=/glibc
# rm -r /usr/share
# rm /bin/git
# cp -r ./usr/share /usr
# cp ./usr/bin/* /bin
# ./git_testcode.sh
# ./gitnetwork_testcode.sh

echo -e "\n================ alpine ================\n"
export PATH="/bin:/sbin:/usr/bin:/usr/sbin"
export LD_LIBRARY_PATH="/lib:/usr/lib"
export HOME=/home
cd /home
ls /home/proj 
cat /home/proj/README.md

# git clone http://20.205.243.166/oscomp/xv6-riscv.git my-repo
# sh

sh
# vim -u NONE hello.c





# ==================================================================#

# echo -e "\n=============== Build RootFS ===============\n"
# 
# /bin/busybox --install -s /bin
# export PATH=/bin
# export LD_LIBRARY_PATH=".:./lib:/musl/lib:/lib"
# mkdir -v -p /var/tmp
# ln -v -s /lib /lib64
# ln -v -s /lib /usr/lib
# ln -v -s /lib /usr/lib64
# 
# echo -e "\n================ Linux APPs ================\n"

# echo -e "\n================ Redis ================\n"
# 
# redis-server --bind 0.0.0.0 &
# sleep 2
# redis-cli

# echo -e "\n================ Bash ================\n"
# bash

# echo -e "\n================ gcc ================\n"
# cd /home
# gcc main.c
#
