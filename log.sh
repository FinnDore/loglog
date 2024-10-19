while true
do
    EPOCH_TIMESTAMP=$(($(date +%s)*1000))
    LOG=$(curl -sb --get https://whatthecommit.com/index.txt | tr -d \" | tr -d \')
    aws logs put-log-events \
        --log-group-name "/service/dev/somelogs" \
        --log-stream-name "yes" \
        --log-events timestamp=$EPOCH_TIMESTAMP,message="${LOG}"
    sleep $((RANDOM % 5)) 
done