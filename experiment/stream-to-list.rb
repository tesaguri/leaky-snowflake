#!/usr/bin/env ruby
# frozen_string_literal: true

# = stream-to-list.rb
#
# Reads Tweets from the Sample Stream and adds their authors to a List.

list = Integer(ARGV[0])

require 'logger'
require 'net/http'
require 'set'

require 'twitter'
require 'twurl'

MAX_LIST_LENGTH = 5000

logger = Logger.new(STDERR)

twurlrc = Twurl::RCFile.new
(username, consumer_key) = twurlrc.default_profile
bearer = twurlrc.bearer_tokens.fetch(consumer_key)

rest = Twitter::REST::Client.new do |client|
  profile = twurlrc.profiles.fetch(username).fetch(consumer_key)
  client.consumer_key = profile.fetch('consumer_key')
  client.consumer_secret = profile.fetch('consumer_secret')
  client.access_token = profile.fetch('token')
  client.access_token_secret = profile.fetch('secret')
end

users = rest.list_members(list, count: MAX_LIST_LENGTH).each_with_object(Set.new) do |user, set|
  set.add user.id
end

def finish
  puts "Collected #{MAX_LIST_LENGTH} users"
  exit
end

finish if users.length >= MAX_LIST_LENGTH

stack = []
current_user = nil
yielder = nil

uri = URI('https://api.twitter.com/2/tweets/sample/stream?tweet.fields=author_id')
request = Net::HTTP::Get.new(uri)
request['Authorization'] = "Bearer #{bearer}"
define_singleton_method('connect_stream') do
  return if yielder&.alive? and not yielder.status == 'aborting'

  yielder = Thread.new do
    Net::HTTP.start(uri.hostname, uri.port, use_ssl: uri.scheme == 'https') do |http|
      http.request(request) do |response|
        response.value
        line = String.new
        response.read_body do |chunk|
          while i = chunk.index("\n")
            line.concat(chunk.slice!(..i))
            line.strip!
            next if line.empty?

            begin
              begin
                json = JSON.parse(line)
              rescue => e
                logger.error <<EOS
Unable to parse stream message: #{e.class.name} (#{e.message})
input=#{line}
EOS
                next
              end

              unless user = Integer(json&.[]('data')&.[]('author_id'), 10, exception: false)
                logger.error "Unknown stream message: #{line}"
                next
              end

              unless [users, stack, [current_user]].any? {|enum| enum.include? user }
                stack.push user
                if stack.length > MAX_LIST_LENGTH - users.length
                  # Prevent the stack from having excessive # of elements while sleeping
                  stack.shift
                end
                Thread.main.run
              end
            ensure
              line = String.new
            end
          end
          line.concat(chunk)
        end
      end
    end
  end
  yielder.abort_on_exception = true

  nil
end

KEEP_ALIVE = 5 * 60

rejection_count = 0
until users.length >= MAX_LIST_LENGTH
  unless current_user = stack.pop
    connect_stream
    sleep until current_user = stack.pop
  end

  begin
    rest.add_list_member(list, current_user)
    logger.info "Added to the List: #{current_user}"
    users.add current_user
    rejection_count = 0
  rescue Twitter::Error::TooManyRequests => e
    wait = e.rate_limit.reset_in
    yielder.kill if wait > KEEP_ALIVE
    logger.info "Will retry in #{wait} seconds..."
    loop do
      sleep wait
      break if (wait = e.rate_limit.reset_in) <= 0
    end
    # Dropping the current user as we're going to have a bunch of new users anyway
    next
  rescue Twitter::Error => e
    logger.error "Unable to add user `#{current_user}` to the List: #{e.class.name} (#{e.message})"
    if e.is_a? Twitter::Error::Forbidden
      # Twitter seems to reject bulk additions to a List with 403
      wait = 1 << rejection_count
      yielder.kill if wait > KEEP_ALIVE
      retry_at = Time.now + wait
      logger.info "Will retry in #{wait} seconds..."
      loop do
        sleep wait.ceil
        break if (wait = retry_at - Time.now) <= 0
      end
      rejection_count += 1
    end
    next
  end

  # Reconnect to the stream on successful response: we'll likely need fresh users then
  connect_stream
end

finish
